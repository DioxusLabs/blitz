use std::collections::HashMap;

use blitz_dom::BaseDocument;
use blitz_traits::node_id::NodeId;
use cssparser::ToCss as _;
use serde_json::json;
use style::computed_values::position::T as Position;
use style::properties::generated::longhands::box_sizing::computed_value::T as BoxSizing;
use style::properties::{
    LonghandId, NonCustomPropertyId, PropertyDeclarationBlock, PropertyDeclarationId, PropertyId,
};
use style::rule_tree::CascadeOrigin;
use style::stylesheets::{CssRule, Origin};

use crate::JsonValue;

/// A single declaration in a matched rule
struct Declaration {
    name: String,
    value: String,
    important: bool,
}

/// A matched style rule, gathered with document access and serialized to
/// CDP JSON afterwards
struct MatchedRule {
    selector_text: String,
    is_user_agent: bool,
    declarations: Vec<Declaration>,
}

/// Metadata about the style rule that a declaration block came from,
/// obtained by walking the document's stylesheets
struct RuleMetadata {
    selector_text: String,
    origin: Origin,
}

/// Build a map from declaration-block pointer to rule metadata by walking
/// all of the document's stylesheets
fn rule_metadata(doc: &BaseDocument) -> HashMap<*const std::ffi::c_void, RuleMetadata> {
    let guard_holder = doc.guard();
    let guard = guard_holder.read();

    let mut map = HashMap::new();

    fn visit_rules(
        rules: &[CssRule],
        guard: &style::shared_lock::SharedRwLockReadGuard,
        origin: Origin,
        map: &mut HashMap<*const std::ffi::c_void, RuleMetadata>,
    ) {
        for rule in rules {
            match rule {
                CssRule::Style(locked_rule) => {
                    let style_rule = locked_rule.read_with(guard);
                    let key = style_rule.block.heap_ptr();
                    map.insert(
                        key,
                        RuleMetadata {
                            selector_text: style_rule.selectors.to_css_string(),
                            origin,
                        },
                    );
                    if let Some(nested) = &style_rule.rules {
                        visit_rules(&nested.read_with(guard).0, guard, origin, map);
                    }
                }
                CssRule::Media(media_rule) => {
                    visit_rules(&media_rule.rules.read_with(guard).0, guard, origin, map);
                }
                CssRule::Supports(supports_rule) => {
                    visit_rules(&supports_rule.rules.read_with(guard).0, guard, origin, map);
                }
                _ => {}
            }
        }
    }

    for (origin, sheet) in doc
        .author_stylesheets()
        .map(|sheet| (Origin::Author, sheet))
        .chain(
            doc.useragent_stylesheets()
                .map(|sheet| (Origin::UserAgent, sheet)),
        )
    {
        let contents = sheet.0.contents.read_with(&guard);
        let rules = contents.rules.read_with(&guard);
        visit_rules(&rules.0, &guard, origin, &mut map);
    }

    map
}

/// Extract the declarations from a declaration block, optionally keeping
/// only inherited properties
fn block_declarations(block: &PropertyDeclarationBlock, inherited_only: bool) -> Vec<Declaration> {
    block
        .declaration_importance_iter()
        .filter_map(|(decl, importance)| {
            let id = decl.id();
            if inherited_only {
                let is_inherited = match id {
                    PropertyDeclarationId::Longhand(longhand) => longhand.inherited(),
                    PropertyDeclarationId::Custom(_) => true,
                };
                if !is_inherited {
                    return None;
                }
            }
            let mut value = String::new();
            decl.to_css(&mut value).ok()?;
            Some(Declaration {
                name: id.name().to_string(),
                value,
                important: importance.important(),
            })
        })
        .collect()
}

/// Serialize declarations to a CDP `CSS.CSSStyle` object
fn css_style_json(declarations: &[Declaration]) -> JsonValue {
    let properties: Vec<JsonValue> = declarations
        .iter()
        .map(|decl| {
            json!({
                "name": decl.name,
                "value": decl.value,
                "important": decl.important,
                "implicit": false,
                "disabled": false,
                "text": if decl.important {
                    format!("{}: {} !important;", decl.name, decl.value)
                } else {
                    format!("{}: {};", decl.name, decl.value)
                },
            })
        })
        .collect();
    let css_text: String = declarations
        .iter()
        .map(|decl| {
            if decl.important {
                format!("{}: {} !important; ", decl.name, decl.value)
            } else {
                format!("{}: {}; ", decl.name, decl.value)
            }
        })
        .collect();
    json!({
        "cssProperties": properties,
        "shorthandEntries": [],
        "cssText": css_text,
    })
}

/// Serialize a matched rule to a CDP `CSS.RuleMatch` object
fn rule_match_json(rule: &MatchedRule) -> JsonValue {
    // An empty selector confuses the frontend's rule display: fall back to "*"
    let selector_text = if rule.selector_text.is_empty() {
        "*".to_string()
    } else {
        rule.selector_text.clone()
    };
    let selectors: Vec<JsonValue> = selector_text
        .split(',')
        .map(|sel| json!({ "text": sel.trim() }))
        .collect();
    let matching: Vec<usize> = (0..selectors.len()).collect();
    json!({
        "rule": {
            "selectorList": { "selectors": selectors, "text": selector_text },
            "origin": if rule.is_user_agent { "user-agent" } else { "regular" },
            "style": css_style_json(&rule.declarations),
        },
        "matchingSelectors": matching,
    })
}

/// Collect the style rules matching a node from Stylo's rule tree,
/// in ascending cascade order (least to most important, as CDP expects).
/// The inline style attribute is excluded (it is reported separately).
fn matched_rules(
    doc: &BaseDocument,
    node_id: NodeId,
    metadata: &HashMap<*const std::ffi::c_void, RuleMetadata>,
    inherited_only: bool,
) -> Vec<MatchedRule> {
    let guard_holder = doc.guard();
    let guard = guard_holder.read();

    let Some(node) = doc.get_node(node_id) else {
        return Vec::new();
    };

    let mut rules = Vec::new();

    if let Some(styles) = node.primary_styles()
        && let Some(rule_node) = &styles.rules
    {
        for rule_node in rule_node.self_and_ancestors() {
            let Some(source) = rule_node.style_source() else {
                continue;
            };
            let block = source.read(&guard);

            // Skip the inline style attribute (reported separately)
            let is_style_attribute = node
                .element_data()
                .and_then(|element| element.style_attribute.as_ref())
                .is_some_and(|style_attr| style::servo_arc::Arc::ptr_eq(source.get(), style_attr));
            if is_style_attribute {
                continue;
            }

            let declarations = block_declarations(block, inherited_only);
            if declarations.is_empty() {
                continue;
            }

            let origin_is_ua = rule_node.cascade_level().origin() == CascadeOrigin::UA;
            let key = source.get().heap_ptr();
            let meta = metadata.get(&key);

            rules.push(MatchedRule {
                selector_text: meta.map(|m| m.selector_text.clone()).unwrap_or_default(),
                is_user_agent: meta
                    .map(|m| m.origin == Origin::UserAgent)
                    .unwrap_or(origin_is_ua),
                declarations,
            });
        }
    }

    // Stylo's rule tree iterates from most to least important: reverse to
    // get the ascending cascade order CDP expects
    rules.reverse();
    rules
}

/// The declarations of a node's inline `style` attribute
fn inline_declarations(
    doc: &BaseDocument,
    node_id: NodeId,
    inherited_only: bool,
) -> Vec<Declaration> {
    let guard_holder = doc.guard();
    let guard = guard_holder.read();
    doc.get_node(node_id)
        .and_then(|node| node.element_data())
        .and_then(|element| element.style_attribute.as_ref())
        .map(|style_attr| block_declarations(style_attr.read_with(&guard), inherited_only))
        .unwrap_or_default()
}

/// A declaration scanned from authored inline style text, with the byte
/// range of its text within it
struct AuthoredDeclaration {
    name: String,
    value: String,
    important: bool,
    disabled: bool,
    start: usize,
    end: usize,
}

/// Parse `name: value` declaration text into an [`AuthoredDeclaration`]
/// covering the given source range
fn authored_declaration(
    text: &str,
    start: usize,
    end: usize,
    disabled: bool,
) -> Option<AuthoredDeclaration> {
    let (name, value) = text.split_once(':')?;
    let value = value.trim();
    let (value, important) = match value
        .strip_suffix("!important")
        .or_else(|| value.strip_suffix("! important"))
    {
        Some(value) => (value.trim_end(), true),
        None => (value, false),
    };
    Some(AuthoredDeclaration {
        name: name.trim().to_string(),
        value: value.to_string(),
        important,
        disabled,
        start,
        end,
    })
}

/// Scan authored CSS declaration text (a `style` attribute value) into
/// declarations, preserving the authored text (shorthands are not
/// expanded). Declarations commented out as `/* name: value; */` — which is
/// how DevTools disables a declaration via its checkbox — are reported as
/// disabled properties.
fn parse_authored_declarations(text: &str) -> Vec<AuthoredDeclaration> {
    let bytes = text.as_bytes();
    let mut decls = Vec::new();
    let mut seg_start = 0;
    let mut paren_depth = 0i32;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i <= bytes.len() {
        let byte = bytes.get(i).copied();
        match (quote, byte) {
            (Some(q), Some(b)) => {
                if b == q {
                    quote = None;
                }
            }
            (None, Some(b @ (b'"' | b'\''))) => quote = Some(b),
            (None, Some(b'(')) => paren_depth += 1,
            (None, Some(b')')) => paren_depth -= 1,
            // A comment: if it wraps a declaration, report it as disabled
            (None, Some(b'/')) if paren_depth <= 0 && text[i..].starts_with("/*") => {
                let len = text[i + 2..].find("*/").map(|p| p + 4);
                let end = i + len.unwrap_or(text.len() - i);
                let inner = text[i + 2..end - if len.is_some() { 2 } else { 0 }].trim();
                let inner = inner.strip_suffix(';').unwrap_or(inner).trim_end();
                decls.extend(authored_declaration(inner, i, end, true));
                i = end;
                seg_start = i;
                continue;
            }
            (None, Some(b';')) | (None, None) if paren_depth <= 0 => {
                let segment = &text[seg_start..i];
                let trimmed = segment.trim();
                let start = seg_start + (segment.len() - segment.trim_start().len());
                // The declaration's text includes the terminating `;`
                let end = if byte.is_some() {
                    i + 1
                } else {
                    start + trimmed.len()
                };
                decls.extend(authored_declaration(trimmed, start, end, false));
                seg_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    decls
}

/// The line/column position of a byte offset within the given text
fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset];
    let line = prefix.matches('\n').count();
    let column = offset - prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    (line, column)
}

/// A CDP `CSS.SourceRange` for the given byte range within the text
fn source_range_json(text: &str, start: usize, end: usize) -> JsonValue {
    let (start_line, start_column) = line_col(text, start);
    let (end_line, end_column) = line_col(text, end);
    json!({
        "startLine": start_line,
        "startColumn": start_column,
        "endLine": end_line,
        "endColumn": end_column,
    })
}

/// Build the `CSS.getInlineStylesForNode` inline style object for a node.
/// The style carries a synthetic per-element style sheet id, the authored
/// `style` attribute text (shorthands are not expanded: DevTools re-commits
/// the reported text while editing, so it must round-trip verbatim) and
/// source ranges into it, making it editable via `CSS.setStyleTexts`.
pub(crate) fn inline_style_json(doc: &BaseDocument, node_id: NodeId) -> JsonValue {
    let text = inline_style_text(doc, node_id);
    let properties: Vec<JsonValue> = parse_authored_declarations(&text)
        .iter()
        .map(|decl| {
            json!({
                "name": decl.name,
                "value": decl.value,
                "important": decl.important,
                "implicit": false,
                "disabled": decl.disabled,
                "text": &text[decl.start..decl.end],
                "range": source_range_json(&text, decl.start, decl.end),
            })
        })
        .collect();
    json!({
        "cssProperties": properties,
        "shorthandEntries": [],
        "cssText": text,
        "range": source_range_json(&text, 0, text.len()),
        "styleSheetId": inline_style_sheet_id(node_id),
    })
}

/// The text backing an element's synthetic inline style sheet: the authored
/// value of its `style` attribute
pub(crate) fn inline_style_text(doc: &BaseDocument, node_id: NodeId) -> String {
    doc.get_node(node_id)
        .and_then(|node| node.attr(blitz_dom::local_name!("style")))
        .unwrap_or_default()
        .to_string()
}

/// The synthetic style sheet id of an element's inline `style` attribute
pub(crate) fn inline_style_sheet_id(node_id: NodeId) -> String {
    format!("inline-{}", crate::session::cdp_node_id(node_id))
}

/// Parse a synthetic inline style sheet id back to its element's node id
pub(crate) fn parse_inline_style_sheet_id(id: &str) -> Option<NodeId> {
    id.strip_prefix("inline-")
        .and_then(|id| id.parse::<u64>().ok())
        .and_then(crate::session::blitz_node_id)
}

/// Build the `CSS.getMatchedStylesForNode` response for a node
pub(crate) fn matched_styles_json(doc: &BaseDocument, node_id: NodeId) -> Option<JsonValue> {
    let node = doc.get_node(node_id)?;
    let metadata = rule_metadata(doc);

    let matched: Vec<JsonValue> = matched_rules(doc, node_id, &metadata, false)
        .iter()
        .map(rule_match_json)
        .collect();

    // Inherited styles: one entry per ancestor, from the direct parent upward
    let mut inherited = Vec::new();
    let mut current = node.parent;
    while let Some(ancestor_id) = current {
        let Some(ancestor) = doc.get_node(ancestor_id) else {
            break;
        };
        if ancestor.element_data().is_some() {
            let rules: Vec<JsonValue> = matched_rules(doc, ancestor_id, &metadata, true)
                .iter()
                .map(rule_match_json)
                .collect();
            let inline = inline_declarations(doc, ancestor_id, true);
            let mut entry = json!({ "matchedCSSRules": rules });
            if !inline.is_empty() {
                entry["inlineStyle"] = css_style_json(&inline);
            }
            inherited.push(entry);
        }
        current = ancestor.parent;
    }

    Some(json!({
        "inlineStyle": inline_style_json(doc, node_id),
        "attributesStyle": null,
        "matchedCSSRules": matched,
        "inherited": inherited,
        "pseudoElements": [],
        "inheritedPseudoElements": [],
        "cssKeyframesRules": [],
    }))
}

/// The used (post-layout) values for the box properties, in px, keyed by
/// property name. Browsers report used values for these in computed style
/// (e.g. `width: auto` computes to the laid-out pixel width), and the
/// DevTools Box Model diagram is built from them.
fn used_box_values(doc: &BaseDocument, node_id: NodeId) -> HashMap<&'static str, String> {
    let mut map = HashMap::new();

    let Some(node) = doc.get_node(node_id) else {
        return map;
    };
    if node.element_data().is_none() {
        return map;
    }
    let Some(rect) = doc.get_client_bounding_rect(node_id) else {
        return map;
    };

    let px = |value: f64| format!("{}px", (value * 100.0).round() / 100.0);

    // Non-atomic inline elements have no layout box of their own: report
    // the bounding rect of their line-box fragments and zero box insets
    let is_inline_fragment = doc.inline_fragment_rects(node_id).is_some();
    let layout = node.final_layout();
    let (border, padding, margin) = if is_inline_fragment {
        Default::default()
    } else {
        (layout.border, layout.padding, layout.margin)
    };

    // Like Chrome, the reported width/height are box-sizing aware: the
    // border-box size for `box-sizing: border-box` elements, the content-box
    // size otherwise
    let is_border_box = node
        .primary_styles()
        .is_some_and(|styles| styles.get_position().box_sizing == BoxSizing::BorderBox);
    let (width, height) = if is_border_box {
        (rect.width, rect.height)
    } else {
        (
            rect.width - (border.left + border.right + padding.left + padding.right) as f64,
            rect.height - (border.top + border.bottom + padding.top + padding.bottom) as f64,
        )
    };

    map.insert("width", px(width.max(0.0)));
    map.insert("height", px(height.max(0.0)));
    map.insert("margin-top", px(margin.top as f64));
    map.insert("margin-right", px(margin.right as f64));
    map.insert("margin-bottom", px(margin.bottom as f64));
    map.insert("margin-left", px(margin.left as f64));
    map.insert("padding-top", px(padding.top as f64));
    map.insert("padding-right", px(padding.right as f64));
    map.insert("padding-bottom", px(padding.bottom as f64));
    map.insert("padding-left", px(padding.left as f64));
    map.insert("border-top-width", px(border.top as f64));
    map.insert("border-right-width", px(border.right as f64));
    map.insert("border-bottom-width", px(border.bottom as f64));
    map.insert("border-left-width", px(border.left as f64));

    // Inset properties resolve to used px values for positioned elements
    // (the offsets from the parent's padding box); for statically
    // positioned elements they compute to `auto`, as in browsers
    let is_positioned = node
        .primary_styles()
        .is_some_and(|styles| styles.get_box().position != Position::Static);
    if is_positioned
        && !is_inline_fragment
        && let Some(parent) = node.parent.and_then(|parent_id| doc.get_node(parent_id))
    {
        let parent_layout = parent.final_layout();
        let loc = layout.location;
        let top = (loc.y - parent_layout.border.top) as f64;
        let left = (loc.x - parent_layout.border.left) as f64;
        let right = (parent_layout.size.width
            - parent_layout.border.right
            - (loc.x + layout.size.width)) as f64;
        let bottom = (parent_layout.size.height
            - parent_layout.border.bottom
            - (loc.y + layout.size.height)) as f64;
        map.insert("top", px(top));
        map.insert("left", px(left));
        map.insert("right", px(right));
        map.insert("bottom", px(bottom));
    }

    map
}

/// Build the `CSS.getComputedStyleForNode` property list for a node: every
/// longhand property serialized from the node's computed style, with used
/// (post-layout) values substituted for the box properties
pub(crate) fn computed_style_json(doc: &BaseDocument, node_id: NodeId) -> Option<JsonValue> {
    let node = doc.get_node(node_id)?;
    let styles = node.primary_styles()?;
    let used = used_box_values(doc, node_id);

    let mut computed = Vec::new();
    for id in NonCustomPropertyId::iter() {
        let Ok(longhand) = id.longhand_or_shorthand() else {
            continue;
        };
        if id.as_alias().is_some() {
            continue;
        }
        if !PropertyId::NonCustom(id).enabled_for_all_content() {
            continue;
        }
        let name = longhand.name();
        let value = match used.get(name) {
            Some(used_value) => used_value.clone(),
            None => styles.computed_value_to_string(PropertyDeclarationId::Longhand(longhand)),
        };
        computed.push(json!({ "name": name, "value": value }));
    }
    Some(JsonValue::Array(computed))
}

/// Serialize a node's computed `display` value (used by tests)
#[allow(dead_code)]
pub(crate) fn display_string(styles: &style::properties::ComputedValues) -> String {
    styles.computed_value_to_string(PropertyDeclarationId::Longhand(LonghandId::Display))
}
