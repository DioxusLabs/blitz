//! Handlers for the `CSS` domain: matched/computed/inline styles and
//! style sheet editing

use blitz_traits::node_id::NodeId;
use serde_json::json;

use super::{
    CdpError, Session, attr_name, cdp_node_id, no_document, no_node, no_style_sheet, node_id_param,
    str_param, with_doc,
};
use crate::style_sheets::StyleSheets;
use crate::{CdpCommand, DocumentProvider, JsonValue, MessageWriter};

pub(super) fn dispatch(
    session: &mut Session,
    writer: &mut MessageWriter,
    docs: &mut dyn DocumentProvider,
    command: &CdpCommand,
) -> Result<JsonValue, CdpError> {
    let method = command.method.as_str();
    let params = &command.params;
    // Sheets added since the last command are reported before any reply
    // that refers to them
    session.sync_style_sheets(writer, docs);
    match method {
        // Like Chrome, enabling the domain reports every style sheet with a
        // `CSS.styleSheetAdded` event before the reply
        "CSS.enable" => {
            if session.style_sheets.is_none() {
                session.style_sheets = Some(StyleSheets::default());
                session.sync_style_sheets(writer, docs);
            }
            Ok(json!({}))
        }

        "CSS.disable" => {
            session.style_sheets = None;
            Ok(json!({}))
        }

        "CSS.getMatchedStylesForNode" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = node_id_param(params, "nodeId")?;
            let sheets = session.style_sheets.as_mut();
            with_doc(docs, doc_id, |doc| {
                crate::css::matched_styles_json(doc, node_id, sheets)
            })
            .flatten()
            .ok_or_else(no_node)
        }

        "CSS.getComputedStyleForNode" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = node_id_param(params, "nodeId")?;
            let computed = with_doc(docs, doc_id, |doc| {
                crate::css::computed_style_json(doc, node_id)
            })
            .flatten()
            .ok_or_else(no_node)?;
            Ok(json!({ "computedStyle": computed }))
        }

        "CSS.getInlineStylesForNode" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = node_id_param(params, "nodeId")?;
            let inline = with_doc(docs, doc_id, |doc| {
                crate::css::inline_style_json(doc, node_id)
            })
            .ok_or_else(no_node)?;
            Ok(json!({ "inlineStyle": inline, "attributesStyle": null }))
        }

        "CSS.getPlatformFontsForNode" => Ok(json!({ "fonts": [] })),

        "CSS.getStyleSheetText" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let sheet_id = str_param(params, "styleSheetId")?;
            if let Some(text) = session
                .style_sheets
                .as_ref()
                .and_then(|sheets| sheets.text(&sheet_id))
            {
                return Ok(json!({ "text": text }));
            }
            let node_id =
                crate::css::parse_inline_style_sheet_id(&sheet_id).ok_or_else(no_style_sheet)?;
            let text = with_doc(docs, doc_id, |doc| {
                doc.get_node(node_id)
                    .filter(|node| node.element_data().is_some())
                    .ok_or_else(no_style_sheet)?;
                Ok(crate::css::inline_style_text(doc, node_id))
            })
            .ok_or_else(no_document)??;
            Ok(json!({ "text": text }))
        }

        // Sent by the Styles pane when a style is edited. Only inline
        // styles (per-element synthetic style sheets) are editable: each
        // edit's text replaces the element's `style` attribute
        "CSS.setStyleTexts" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let edits = params
                .get("edits")
                .and_then(|edits| edits.as_array())
                .ok_or_else(|| CdpError::invalid_params("Missing edits"))?
                .clone();
            // All the edits' ranges refer to the same original snapshot
            // of each sheet's text, so resolve every range against that
            // snapshot before applying any of them
            struct SheetEdits {
                sheet_id: String,
                node_id: NodeId,
                text: String,
                // (start, end) byte offsets into `text`, with replacements
                splices: Vec<(usize, usize, String)>,
            }
            let mut sheets: Vec<SheetEdits> = Vec::new();
            let mut edit_sheets = Vec::new();
            // Edits to rules of reported style sheets, as (edit index,
            // sheet id, splice)
            let mut rule_edits: Vec<(usize, String, (usize, usize, String))> = Vec::new();
            for (index, edit) in edits.iter().enumerate() {
                let sheet_id = str_param(edit, "styleSheetId")?;
                let text = str_param(edit, "text")?;
                let range = edit
                    .get("range")
                    .ok_or_else(|| CdpError::invalid_params("Missing range"))?;
                if let Some(sheet_text) = session
                    .style_sheets
                    .as_ref()
                    .and_then(|style_sheets| style_sheets.text(&sheet_id))
                {
                    let (start, end) = range_offsets(sheet_text, range)
                        .ok_or_else(|| CdpError::invalid_params("Invalid range"))?;
                    rule_edits.push((index, sheet_id, (start, end, text)));
                    continue;
                }
                let node_id = crate::css::parse_inline_style_sheet_id(&sheet_id)
                    .ok_or_else(no_style_sheet)?;
                let sheet = match sheets.iter_mut().find(|s| s.node_id == node_id) {
                    Some(sheet) => sheet,
                    None => {
                        let sheet_text = with_doc(docs, doc_id, |doc| {
                            doc.get_node(node_id)
                                .filter(|node| node.element_data().is_some())
                                .ok_or_else(no_style_sheet)?;
                            Ok(crate::css::inline_style_text(doc, node_id))
                        })
                        .ok_or_else(no_document)??;
                        sheets.push(SheetEdits {
                            sheet_id: sheet_id.clone(),
                            node_id,
                            text: sheet_text,
                            splices: Vec::new(),
                        });
                        sheets.last_mut().unwrap()
                    }
                };
                let (start, end) = range_offsets(&sheet.text, range)
                    .ok_or_else(|| CdpError::invalid_params("Invalid range"))?;
                if sheet.splices.iter().any(|&(s, e, _)| start < e && s < end) {
                    return Err(CdpError::invalid_params("Overlapping ranges"));
                }
                sheet.splices.push((start, end, text));
                edit_sheets.push(node_id);
            }
            let mut modified = Vec::new();
            let mut sheet_styles = Vec::new();
            for sheet in &mut sheets {
                // Apply back-to-front so earlier splices don't shift the
                // offsets of later ones
                sheet.splices.sort_by_key(|s| (s.0, s.1));
                let mut new_attr = sheet.text.clone();
                for (start, end, text) in sheet.splices.iter().rev() {
                    new_attr.replace_range(start..end, text);
                }
                let node_id = sheet.node_id;
                let (style, value) = with_doc(docs, doc_id, |doc| {
                    doc.mutate()
                        .set_attribute(node_id, attr_name("style"), &new_attr);
                    doc.shell_provider.request_redraw();
                    // Report the style re-serialized from the parsed
                    // attribute, so its ranges match the new sheet text
                    (
                        crate::css::inline_style_json(doc, node_id),
                        crate::css::inline_style_text(doc, node_id),
                    )
                })
                .ok_or_else(no_document)?;
                sheet_styles.push((node_id, style));
                modified.push((sheet.sheet_id.clone(), node_id, value));
            }
            // One resulting style per edit, in the edits' order (edits to
            // the same sheet all report that sheet's final style)
            let mut styles: Vec<JsonValue> = edit_sheets
                .iter()
                .map(|node_id| {
                    sheet_styles
                        .iter()
                        .find(|(id, _)| id == node_id)
                        .map(|(_, style)| style.clone())
                        .unwrap()
                })
                .collect();

            // Rule edits splice the DevTools copy of the sheet text and
            // replace the edited rules' declarations. Like Chrome, no
            // `CSS.styleSheetChanged` is sent to the session that made them.
            let mut rule_sheet_ids: Vec<&str> =
                rule_edits.iter().map(|(_, id, _)| id.as_str()).collect();
            rule_sheet_ids.dedup();
            let mut rule_styles = Vec::new();
            for sheet_id in rule_sheet_ids {
                let (indices, splices): (Vec<usize>, Vec<_>) = rule_edits
                    .iter()
                    .filter(|(_, id, _)| id == sheet_id)
                    .map(|(index, _, splice)| (*index, splice.clone()))
                    .unzip();
                let style_sheets = session.style_sheets.as_mut().unwrap();
                let results = with_doc(docs, doc_id, |doc| {
                    let results = style_sheets.edit_rule_styles(doc, sheet_id, &splices);
                    doc.shell_provider.request_redraw();
                    results
                })
                .ok_or_else(no_document)?
                .map_err(CdpError::server_error)?;
                for (index, (text, (start, end))) in indices.into_iter().zip(results) {
                    let style = crate::css::authored_style_json(&text, start, end, sheet_id);
                    rule_styles.push((index, style));
                }
            }
            // Merge the rule edits' styles back in at their edits' positions
            rule_styles.sort_by_key(|(index, _)| *index);
            for (index, style) in rule_styles {
                styles.insert(index, style);
            }
            for (sheet_id, node_id, value) in &modified {
                writer.event("CSS.styleSheetChanged", json!({ "styleSheetId": sheet_id }));
                writer.event(
                    "DOM.attributeModified",
                    json!({ "nodeId": cdp_node_id(*node_id), "name": "style", "value": value }),
                );
            }
            Ok(json!({ "styles": styles }))
        }

        // Sent by the Styles pane when a rule's selector is edited
        "CSS.setRuleSelector" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let sheet_id = str_param(params, "styleSheetId")?;
            let selector = str_param(params, "selector")?;
            let range = params
                .get("range")
                .ok_or_else(|| CdpError::invalid_params("Missing range"))?;
            let style_sheets = session.style_sheets.as_mut().ok_or_else(no_style_sheet)?;
            let text = style_sheets.text(&sheet_id).ok_or_else(no_style_sheet)?;
            let range = range_offsets(text, range)
                .ok_or_else(|| CdpError::invalid_params("Invalid range"))?;
            with_doc(docs, doc_id, |doc| {
                let source = style_sheets.set_rule_selector(doc, &sheet_id, range, &selector)?;
                let selector_list = crate::css::selector_list_json(&source);
                doc.shell_provider.request_redraw();
                Ok(json!({ "selectorList": selector_list }))
            })
            .ok_or_else(no_document)?
            .map_err(CdpError::server_error)
        }

        // Sent by the Styles pane's "Insert style rule below" button
        "CSS.addRule" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let sheet_id = str_param(params, "styleSheetId")?;
            let rule_text = str_param(params, "ruleText")?;
            let location = params
                .get("location")
                .ok_or_else(|| CdpError::invalid_params("Missing location"))?;
            let style_sheets = session.style_sheets.as_mut().ok_or_else(no_style_sheet)?;
            let text = style_sheets.text(&sheet_id).ok_or_else(no_style_sheet)?;
            let (location, _) = range_offsets(text, location)
                .ok_or_else(|| CdpError::invalid_params("Invalid location"))?;
            with_doc(docs, doc_id, |doc| {
                let source = style_sheets.add_rule(doc, &sheet_id, location, &rule_text)?;
                let rule = crate::css::sheet_rule_json(&source);
                doc.shell_provider.request_redraw();
                Ok(json!({ "rule": rule }))
            })
            .ok_or_else(no_document)?
            .map_err(CdpError::server_error)
        }

        _ => Err(CdpError::method_not_found(method)),
    }
}

/// Convert a CDP `{startLine, startColumn, endLine, endColumn}` source range
/// into byte offsets within the given text
fn range_offsets(text: &str, range: &JsonValue) -> Option<(usize, usize)> {
    let position = |line: u64, column: u64| -> Option<usize> {
        let mut offset = 0;
        for _ in 0..line {
            offset += text[offset..].find('\n')? + 1;
        }
        let offset = offset + column as usize;
        (offset <= text.len() && text.is_char_boundary(offset)).then_some(offset)
    };
    let get = |key: &str| range.get(key).and_then(|v| v.as_u64());
    let start = position(get("startLine")?, get("startColumn")?)?;
    let end = position(get("endLine")?, get("endColumn")?)?;
    (start <= end).then_some((start, end))
}
