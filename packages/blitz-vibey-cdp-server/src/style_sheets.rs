//! DevTools-side copies of a document's author style sheets, which make
//! the rules of `<style>` and `<link>` sheets editable from the Styles pane.
//!
//! Like Chrome, each sheet's text is held here rather than in the document:
//! edits made through DevTools splice this text and are applied to the
//! document's CSSOM rule by rule, leaving the owner element unchanged.

use blitz_dom::BaseDocument;
use blitz_dom::node::SpecialElementData;
use blitz_traits::node_id::NodeId;
use cssparser::{ParseError, Parser, SourceLocation, ToCss as _, Token};
use serde_json::json;
use style::context::QuirksMode;
use style::media_queries::MediaList;
use style::servo_arc::Arc as ServoArc;
use style::shared_lock::{Locked, SharedRwLock, SharedRwLockReadGuard, ToCssWithGuard};
use style::stylesheets::import_rule::{ImportLayer, ImportSheet, ImportSupportsCondition};
use style::stylesheets::{
    AllowImportRules, CssRule, CssRules, DocumentStyleSheet, ImportRule, Origin, Stylesheet,
    StylesheetLoader, UrlExtraData,
};
use style::values::CssUrl;

use crate::{JsonValue, MessageWriter};

/// A byte range of a sheet's text
pub(crate) type TextRange = (usize, usize);

/// The style sheets reported to a session that has enabled the `CSS` domain
#[derive(Default)]
pub(crate) struct StyleSheets {
    /// The document's stylesheet generation when the sheets were last synced
    generation: Option<u64>,
    sheets: Vec<SheetEntry>,
    next_id: u64,
}

struct SheetEntry {
    id: String,
    owner: NodeId,
    /// Identity of the document's sheet object: a sheet replaced by Blitz
    /// (e.g. after its `<style>` text changed) is reported as a new sheet
    sheet_ptr: usize,
    text: String,
    /// Built from `text` the first time a rule's ranges are needed
    rules: Option<Vec<RuleRanges>>,
}

/// The source ranges (byte offsets into the sheet text) of a style rule
pub(crate) struct RuleRanges {
    /// The rule's index path in the sheet's rule tree
    pub(crate) path: Vec<usize>,
    /// The selectors as serialized by Stylo, to check that the document's
    /// rule at `path` is still the one this text describes
    selector_text: String,
    /// Each selector of the prelude
    pub(crate) selectors: Vec<TextRange>,
    /// The declarations: the inside of the `{}` block, up to the first
    /// nested rule
    pub(crate) style: TextRange,
}

impl RuleRanges {
    /// The whole selector list of the prelude
    pub(crate) fn selector_list(&self) -> Option<TextRange> {
        Some((self.selectors.first()?.0, self.selectors.last()?.1))
    }
}

/// A style rule located in a reported sheet
pub(crate) struct RuleSource<'a> {
    pub(crate) sheet_id: &'a str,
    pub(crate) text: &'a str,
    pub(crate) ranges: &'a RuleRanges,
}

impl StyleSheets {
    /// Report sheets added to or removed from the document since the last
    /// sync with `CSS.styleSheetAdded` / `CSS.styleSheetRemoved` events
    pub(crate) fn sync(&mut self, doc: &BaseDocument, doc_id: usize, writer: &mut MessageWriter) {
        let generation = doc.stylesheet_generation();
        if self.generation == Some(generation) {
            return;
        }
        self.generation = Some(generation);

        let current: Vec<(NodeId, &DocumentStyleSheet)> = doc
            .stylesheet_owner_nodes()
            .filter_map(|owner| Some((owner, owner_sheet(doc, owner)?)))
            .collect();

        self.sheets.retain(|entry| {
            let kept = current.iter().any(|(owner, sheet)| {
                *owner == entry.owner && sheet_identity(sheet) == entry.sheet_ptr
            });
            if !kept {
                writer.event("CSS.styleSheetRemoved", json!({ "styleSheetId": entry.id }));
            }
            kept
        });

        for (owner, sheet) in current {
            let sheet_ptr = sheet_identity(sheet);
            if self.sheets.iter().any(|entry| entry.sheet_ptr == sheet_ptr) {
                continue;
            }
            self.next_id += 1;
            let entry = SheetEntry {
                id: format!("sheet-{}", self.next_id),
                owner,
                sheet_ptr,
                text: initial_sheet_text(doc, owner, sheet),
                rules: None,
            };
            writer.event(
                "CSS.styleSheetAdded",
                json!({ "header": header_json(doc, doc_id, &entry) }),
            );
            self.sheets.push(entry);
        }
    }

    fn entry(&self, sheet_id: &str) -> Option<&SheetEntry> {
        self.sheets.iter().find(|entry| entry.id == sheet_id)
    }

    fn entry_mut(&mut self, sheet_id: &str) -> Option<&mut SheetEntry> {
        self.sheets.iter_mut().find(|entry| entry.id == sheet_id)
    }

    /// The DevTools text of a reported sheet
    pub(crate) fn text(&self, sheet_id: &str) -> Option<&str> {
        self.entry(sheet_id).map(|entry| entry.text.as_str())
    }

    /// Locate the style rule at `path` in the sheet owned by `owner`, if the
    /// sheet's text still describes it (its selectors match `selector_text`)
    pub(crate) fn rule_source(
        &mut self,
        doc: &BaseDocument,
        owner: NodeId,
        path: &[usize],
        selector_text: &str,
    ) -> Option<RuleSource<'_>> {
        let sheet_ptr = sheet_identity(owner_sheet(doc, owner)?);
        let entry = self
            .sheets
            .iter_mut()
            .find(|entry| entry.owner == owner && entry.sheet_ptr == sheet_ptr)?;
        if entry.rules.is_none() {
            entry.rules = Some(index_rules(doc, &entry.text));
        }
        let ranges = entry
            .rules
            .as_ref()?
            .iter()
            .find(|ranges| ranges.path == path && ranges.selector_text == selector_text)?;
        Some(RuleSource {
            sheet_id: &entry.id,
            text: &entry.text,
            ranges,
        })
    }

    /// Apply `CSS.setStyleTexts` edits to the declarations of rules in the
    /// sheet `sheet_id`. Each splice replaces a byte range of the sheet
    /// text, which must lie within a single rule's declarations. Returns
    /// the new text and ranges of the edited rule for each splice.
    pub(crate) fn edit_rule_styles(
        &mut self,
        doc: &mut BaseDocument,
        sheet_id: &str,
        splices: &[(usize, usize, String)],
    ) -> Result<Vec<(String, TextRange)>, &'static str> {
        let entry = self
            .entry_mut(sheet_id)
            .ok_or("No style sheet with given id found")?;
        let owner = entry.owner;
        if entry.rules.is_none() {
            entry.rules = Some(index_rules(doc, &entry.text));
        }
        let old_rules = entry.rules.as_ref().unwrap();

        // The rule each splice edits
        let mut edited = Vec::new();
        for &(start, end, _) in splices {
            let rule = old_rules
                .iter()
                .find(|rule| rule.style.0 <= start && end <= rule.style.1)
                .ok_or("Range is not within a rule's declarations")?;
            // The document's rule must still be the one the text describes
            if live_selector_text(doc, owner, &rule.path).as_deref()
                != Some(rule.selector_text.as_str())
            {
                return Err("The style sheet was modified");
            }
            edited.push(rule.path.clone());
        }

        let mut sorted: Vec<&(usize, usize, String)> = splices.iter().collect();
        sorted.sort_by_key(|splice| (splice.0, splice.1));
        if sorted.windows(2).any(|pair| pair[1].0 < pair[0].1) {
            return Err("Overlapping ranges");
        }
        let mut new_text = entry.text.clone();
        for (start, end, text) in sorted.iter().rev() {
            new_text.replace_range(*start..*end, text);
        }

        // An edit may only change declarations, not the rules themselves
        let new_rules = index_rules(doc, &new_text);
        let same_rules = new_rules.len() == old_rules.len()
            && new_rules
                .iter()
                .zip(old_rules)
                .all(|(new, old)| new.path == old.path && new.selector_text == old.selector_text);
        if !same_rules {
            return Err("Edit changes the rules of the style sheet");
        }

        let mut applied: Vec<&[usize]> = Vec::new();
        for path in &edited {
            if applied.contains(&path.as_slice()) {
                continue;
            }
            applied.push(path);
            let rule = new_rules.iter().find(|rule| &rule.path == path).unwrap();
            let css = &new_text[rule.style.0..rule.style.1];
            doc.stylesheet_rule_style_set_css_text(owner, path, css)
                .map_err(|_| "Could not edit rule")?;
        }

        let result = edited
            .iter()
            .map(|path| {
                let rule = new_rules.iter().find(|rule| &rule.path == path).unwrap();
                (new_text.clone(), rule.style)
            })
            .collect();
        entry.text = new_text;
        entry.rules = Some(new_rules);
        Ok(result)
    }
    /// Apply a `CSS.setRuleSelector` edit: replace the selector list
    /// spanning `range` of a rule of the sheet `sheet_id` with `selector`.
    /// Returns the edited rule's new source.
    pub(crate) fn set_rule_selector(
        &mut self,
        doc: &mut BaseDocument,
        sheet_id: &str,
        range: TextRange,
        selector: &str,
    ) -> Result<RuleSource<'_>, &'static str> {
        let entry = self
            .entry_mut(sheet_id)
            .ok_or("No style sheet with given id found")?;
        let owner = entry.owner;
        if entry.rules.is_none() {
            entry.rules = Some(index_rules(doc, &entry.text));
        }
        let old_rules = entry.rules.as_ref().unwrap();
        let index = old_rules
            .iter()
            .position(|rule| rule.selector_list() == Some(range))
            .ok_or("Range is not a rule's selector list")?;
        let old_rule = &old_rules[index];
        if live_selector_text(doc, owner, &old_rule.path).as_deref()
            != Some(old_rule.selector_text.as_str())
        {
            return Err("The style sheet was modified");
        }

        let mut new_text = entry.text.clone();
        new_text.replace_range(range.0..range.1, selector);
        // The edit may only change this rule's selectors: an invalid
        // selector list drops the rule, and anything else changes the rules
        let new_rules = index_rules(doc, &new_text);
        let same_rules = new_rules.len() == old_rules.len()
            && new_rules
                .iter()
                .zip(old_rules)
                .enumerate()
                .all(|(i, (new, old))| {
                    new.path == old.path && (i == index || new.selector_text == old.selector_text)
                });
        if !same_rules {
            return Err("Invalid selector");
        }

        let new_rule = &new_rules[index];
        let (start, end) = new_rule.selector_list().ok_or("Invalid selector")?;
        doc.stylesheet_rule_set_selector_text(owner, &new_rule.path, &new_text[start..end])
            .map_err(|_| "Could not set selector")?;
        if live_selector_text(doc, owner, &new_rule.path).as_deref()
            != Some(new_rule.selector_text.as_str())
        {
            return Err("Invalid selector");
        }

        entry.text = new_text;
        let ranges = &entry.rules.insert(new_rules)[index];
        Ok(RuleSource {
            sheet_id: &entry.id,
            text: &entry.text,
            ranges,
        })
    }
    /// Apply a `CSS.addRule` edit: insert `rule_text`, which must hold a
    /// single style rule, at byte `location` of the sheet `sheet_id`.
    /// Returns the inserted rule's source.
    pub(crate) fn add_rule(
        &mut self,
        doc: &mut BaseDocument,
        sheet_id: &str,
        location: usize,
        rule_text: &str,
    ) -> Result<RuleSource<'_>, &'static str> {
        let entry = self
            .entry_mut(sheet_id)
            .ok_or("No style sheet with given id found")?;
        let owner = entry.owner;
        if entry.rules.is_none() {
            entry.rules = Some(index_rules(doc, &entry.text));
        }
        let old_rules = entry.rules.as_ref().unwrap();

        let mut new_text = entry.text.clone();
        new_text.insert_str(location, rule_text);
        let inserted = location..location + rule_text.len();
        let new_rules = index_rules(doc, &new_text);
        let index = new_rules
            .iter()
            .position(|rule| {
                rule.selector_list()
                    .is_some_and(|(start, _)| inserted.contains(&start))
            })
            .ok_or("Invalid rule")?;
        let new_path = &new_rules[index].path;
        let (&new_index, parent) = new_path.split_last().unwrap();

        // The text must add just this rule, leaving the others as they were
        // (with the paths of its later siblings and their children shifted)
        let shifted = |path: &[usize]| {
            let mut path = path.to_vec();
            if path.len() > parent.len()
                && path.starts_with(parent)
                && path[parent.len()] >= new_index
            {
                path[parent.len()] += 1;
            }
            path
        };
        let others = new_rules
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != index)
            .map(|(_, rule)| rule);
        let same_rules = new_rules.len() == old_rules.len() + 1
            && new_rules[index].style.1 <= inserted.end
            && others.zip(old_rules).all(|(new, old)| {
                new.path == shifted(&old.path) && new.selector_text == old.selector_text
            });
        if !same_rules {
            return Err("Invalid rule");
        }

        doc.stylesheet_insert_rule(owner, parent, rule_text, new_index)
            .map_err(|_| "Could not add rule")?;
        if live_selector_text(doc, owner, new_path).as_deref()
            != Some(new_rules[index].selector_text.as_str())
        {
            return Err("Could not add rule");
        }

        entry.text = new_text;
        let ranges = &entry.rules.insert(new_rules)[index];
        Ok(RuleSource {
            sheet_id: &entry.id,
            text: &entry.text,
            ranges,
        })
    }
}

fn sheet_identity(sheet: &DocumentStyleSheet) -> usize {
    &*sheet.0 as *const Stylesheet as usize
}

/// The stylesheet owned by a `<style>` / `<link>` element
pub(crate) fn owner_sheet(doc: &BaseDocument, owner: NodeId) -> Option<&DocumentStyleSheet> {
    match &doc.get_node(owner)?.element_data()?.special_data {
        SpecialElementData::Stylesheet(sheet) => Some(sheet),
        _ => None,
    }
}

fn is_inline(doc: &BaseDocument, owner: NodeId) -> bool {
    doc.get_node(owner)
        .and_then(|node| node.element_data())
        .is_some_and(|element| &*element.name.local == "style")
}

/// The text DevTools starts from for a sheet: what a `<style>` element's
/// sheet was parsed from, or for other (`<link>`) sheets, whose source is
/// not kept, the sheet serialized by Stylo
fn initial_sheet_text(doc: &BaseDocument, owner: NodeId, sheet: &DocumentStyleSheet) -> String {
    if is_inline(doc, owner) {
        return doc.get_node(owner).unwrap().text_content();
    }
    let guard_holder = doc.guard();
    let guard = guard_holder.read();
    let rules = sheet.0.contents.read_with(&guard).rules.read_with(&guard);
    let mut text = String::new();
    for rule in rules.0.iter() {
        let mut css = String::new();
        if ToCssWithGuard::to_css(rule, &guard, &mut css).is_ok() {
            text.push_str(&css);
            text.push('\n');
        }
    }
    text
}

/// A CDP `CSS.CSSStyleSheetHeader` for a sheet
fn header_json(doc: &BaseDocument, doc_id: usize, entry: &SheetEntry) -> JsonValue {
    let is_inline = is_inline(doc, entry.owner);
    let source_url = if is_inline {
        doc.url().to_string()
    } else {
        doc.get_node(entry.owner)
            .and_then(|node| node.attr(blitz_dom::local_name!("href")))
            .and_then(|href| doc.url().join(href).ok())
            .map(|url| url.to_string())
            .unwrap_or_default()
    };
    let (end_line, end_column) = crate::css::line_col(&entry.text, entry.text.len());
    json!({
        "styleSheetId": entry.id,
        "frameId": format!("frame-{doc_id}"),
        "sourceURL": source_url,
        "origin": "regular",
        "title": "",
        "ownerNode": crate::session::cdp_node_id(entry.owner),
        "disabled": doc.stylesheet_disabled(entry.owner),
        "hasSourceURL": false,
        "isInline": is_inline,
        "isMutable": false,
        "isConstructed": false,
        "startLine": 0,
        "startColumn": 0,
        "length": entry.text.len(),
        "endLine": end_line,
        "endColumn": end_column,
    })
}

/// The rule list nested in a rule, if it has one (mirrors the rule paths
/// used by blitz-dom's CSSOM)
fn child_rules(
    rule: &CssRule,
    guard: &SharedRwLockReadGuard,
) -> Option<ServoArc<Locked<CssRules>>> {
    match rule {
        CssRule::Style(r) => r.read_with(guard).rules.clone(),
        CssRule::Media(r) => Some(r.rules.clone()),
        CssRule::Supports(r) => Some(r.rules.clone()),
        CssRule::Container(r) => Some(r.rules.clone()),
        CssRule::Page(r) => Some(r.read_with(guard).rules.clone()),
        CssRule::Document(r) => Some(r.rules.clone()),
        CssRule::LayerBlock(r) => Some(r.rules.clone()),
        CssRule::Scope(r) => Some(r.rules.clone()),
        CssRule::StartingStyle(r) => Some(r.rules.clone()),
        _ => None,
    }
}

/// Visit every style rule of a rule tree with its index path
pub(crate) fn visit_style_rules(
    rules: &CssRules,
    guard: &SharedRwLockReadGuard,
    path: &mut Vec<usize>,
    visit: &mut dyn FnMut(&[usize], &CssRule),
) {
    for (index, rule) in rules.0.iter().enumerate() {
        path.push(index);
        if matches!(rule, CssRule::Style(_)) {
            visit(path, rule);
        }
        if let Some(children) = child_rules(rule, guard) {
            visit_style_rules(children.read_with(guard), guard, path, visit);
        }
        path.pop();
    }
}

/// The serialized selectors of the document's style rule at `path` in the
/// sheet owned by `owner`
fn live_selector_text(doc: &BaseDocument, owner: NodeId, path: &[usize]) -> Option<String> {
    let sheet = owner_sheet(doc, owner)?;
    let guard_holder = doc.guard();
    let guard = guard_holder.read();
    let (last, parents) = path.split_last()?;
    let mut rules = sheet.0.contents.read_with(&guard).rules.clone();
    for &index in parents {
        let child = child_rules(rules.read_with(&guard).0.get(index)?, &guard)?;
        rules = child;
    }
    match rules.read_with(&guard).0.get(*last)? {
        CssRule::Style(rule) => Some(rule.read_with(&guard).selectors.to_css_string()),
        _ => None,
    }
}

/// Resolves `@import` rules without loading them (the sheet text is parsed
/// only to locate its rules)
struct NoImportLoader;

impl StylesheetLoader for NoImportLoader {
    fn request_stylesheet(
        &self,
        url: CssUrl,
        location: SourceLocation,
        lock: &SharedRwLock,
        _media: ServoArc<Locked<MediaList>>,
        supports: Option<ImportSupportsCondition>,
        layer: ImportLayer,
    ) -> ServoArc<Locked<ImportRule>> {
        ServoArc::new(lock.wrap(ImportRule {
            url,
            stylesheet: ImportSheet::new_refused(),
            supports,
            layer,
            source_location: location,
        }))
    }
}

/// Locate the style rules of a sheet's text. The text is parsed with Stylo,
/// so the rule paths line up with those of the document's sheet (which
/// drops the same invalid rules), and each rule's start location is then
/// scanned to find its selector and declaration ranges.
fn index_rules(doc: &BaseDocument, text: &str) -> Vec<RuleRanges> {
    let lock = SharedRwLock::new();
    let url_data: UrlExtraData = doc.url().clone().into();
    let sheet = Stylesheet::from_str(
        text,
        url_data,
        Origin::Author,
        ServoArc::new(lock.wrap(MediaList::empty())),
        lock.clone(),
        Some(&NoImportLoader),
        None,
        QuirksMode::NoQuirks,
        AllowImportRules::Yes,
    );
    let guard = lock.read();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let offset = |location: SourceLocation| -> Option<usize> {
        let line_start = *line_starts.get(location.line as usize)?;
        let mut units = 1;
        for (i, c) in text[line_start..].char_indices() {
            if units >= location.column as usize {
                return Some(line_start + i);
            }
            units += c.len_utf16();
        }
        Some(text.len())
    };

    let mut out = Vec::new();
    let rules = sheet.contents.read_with(&guard).rules.read_with(&guard);
    visit_style_rules(rules, &guard, &mut Vec::new(), &mut |path, rule| {
        let CssRule::Style(rule) = rule else { return };
        let rule = rule.read_with(&guard);
        let Some(start) = offset(rule.source_location) else {
            return;
        };
        let Some((selectors, (block_start, mut block_end))) = scan_rule(text, start) else {
            return;
        };
        // Nested rules are not part of the rule's own declarations
        if let Some(nested) = &rule.rules {
            for nested_rule in nested.read_with(&guard).0.iter() {
                if let Some(nested_start) = rule_location(nested_rule, &guard).and_then(offset)
                    && nested_start > block_start
                {
                    block_end = block_end.min(nested_start);
                }
            }
        }
        out.push(RuleRanges {
            path: path.to_vec(),
            selector_text: rule.selectors.to_css_string(),
            selectors,
            style: (block_start, block_end),
        });
    });
    out
}

/// The start location of rules that can be nested in a style rule
fn rule_location(rule: &CssRule, guard: &SharedRwLockReadGuard) -> Option<SourceLocation> {
    Some(match rule {
        CssRule::Style(r) => r.read_with(guard).source_location,
        CssRule::NestedDeclarations(r) => r.read_with(guard).source_location,
        CssRule::Media(r) => r.source_location,
        CssRule::Supports(r) => r.source_location,
        CssRule::Container(r) => r.source_location,
        CssRule::LayerBlock(r) => r.source_location,
        CssRule::Scope(r) => r.source_location,
        CssRule::StartingStyle(r) => r.source_location,
        _ => return None,
    })
}

/// Scan a qualified rule starting at byte `start`: the ranges of each
/// selector of its prelude, and of the inside of its `{}` block
fn scan_rule(text: &str, start: usize) -> Option<(Vec<TextRange>, TextRange)> {
    let mut parser = Parser::new(&text[start..]);
    let consume_block = |parser: &mut Parser| {
        parser
            .parse_nested_block(|p| {
                while p.next_including_whitespace_and_comments().is_ok() {}
                Ok::<_, ParseError<()>>(())
            })
            .ok()
    };
    let mut selectors = Vec::new();
    let mut selector: Option<(usize, usize)> = None;
    loop {
        let token_start = start + parser.position().byte_index();
        let token = parser
            .next_including_whitespace_and_comments()
            .ok()?
            .clone();
        match token {
            Token::WhiteSpace(_) | Token::Comment(_) => {}
            Token::Comma => selectors.extend(selector.take()),
            Token::CurlyBracketBlock => {
                selectors.extend(selector.take());
                let block_start = start + parser.position().byte_index();
                consume_block(&mut parser)?;
                let after = start + parser.position().byte_index();
                let block_end = if text[..after].ends_with('}') {
                    after - 1
                } else {
                    after
                };
                return Some((selectors, (block_start, block_end)));
            }
            token => {
                if matches!(
                    token,
                    Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock
                ) {
                    consume_block(&mut parser)?;
                }
                let token_end = start + parser.position().byte_index();
                let range = selector.get_or_insert((token_start, token_end));
                range.1 = token_end;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_dom::DocumentConfig;

    fn ranges(text: &str) -> Vec<(Vec<usize>, Vec<&str>, &str)> {
        let doc: BaseDocument =
            blitz_html::HtmlDocument::from_html("<html></html>", DocumentConfig::default()).into();
        index_rules(&doc, text)
            .into_iter()
            .map(|rule| {
                let selectors = rule
                    .selectors
                    .iter()
                    .map(|&(start, end)| &text[start..end])
                    .collect();
                (rule.path, selectors, &text[rule.style.0..rule.style.1])
            })
            .collect()
    }

    #[test]
    fn rule_ranges() {
        let text = "@import url(a.css);\n\
            /* \u{2192} */ .x::before { content: \"\u{2192}\" }\n\
            @bogus { .dropped { color: red } }\n\
            :is(.a, .b) > p , q { color: red;\n  & .n { color: blue } }\n\
            @media screen { @supports (display: grid) { .g { display: grid } } }";
        assert_eq!(
            ranges(text),
            vec![
                (vec![1], vec![".x::before"], " content: \"\u{2192}\" "),
                (vec![2], vec![":is(.a, .b) > p", "q"], " color: red;\n  "),
                (vec![2, 0], vec!["& .n"], " color: blue "),
                (vec![3, 0, 0], vec![".g"], " display: grid "),
            ]
        );
    }
}
