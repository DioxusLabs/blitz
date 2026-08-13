//! Handlers for the `DOM` domain: document/child-node serialization,
//! querying, box model, outer HTML, and attribute/character-data editing

use blitz_dom::NodeData;
use serde_json::json;

use super::{
    CdpError, Session, any_node_id_param, attr_name, blitz_node_id, cdp_node_id, no_document,
    no_node, node_id_param, str_param, with_doc,
};
use crate::{CdpCommand, DocumentProvider, JsonValue, MessageWriter};

pub(super) fn dispatch(
    session: &mut Session,
    writer: &mut MessageWriter,
    docs: &mut dyn DocumentProvider,
    command: &CdpCommand,
) -> Result<JsonValue, CdpError> {
    let method = command.method.as_str();
    let params = &command.params;
    match method {
        "DOM.getDocument" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let depth = params.get("depth").and_then(|d| d.as_i64()).unwrap_or(1);
            session.children_sent.clear();
            let children_sent = &mut session.children_sent;
            let root = with_doc(docs, doc_id, |doc| {
                crate::dom::node_json(doc, doc.root_node().id, depth, children_sent)
            })
            .flatten()
            .ok_or_else(no_document)?;
            Ok(json!({ "root": root }))
        }

        "DOM.requestChildNodes" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = node_id_param(params, "nodeId")?;
            let depth = params.get("depth").and_then(|d| d.as_i64()).unwrap_or(1);
            if session.children_sent.contains(&node_id) {
                // The frontend already knows this node's children:
                // resending them would replace its node objects and
                // break tree state (see `children_sent`)
                return Ok(json!({}));
            }
            let children_sent = &mut session.children_sent;
            let nodes = with_doc(docs, doc_id, |doc| {
                let node = doc.get_node(node_id)?;
                let children: Vec<JsonValue> = crate::dom::dom_children(doc, node)
                    .iter()
                    .filter_map(|child| {
                        crate::dom::node_json(
                            doc,
                            child.id,
                            crate::dom::child_depth(depth),
                            children_sent,
                        )
                    })
                    .collect();
                Some(children)
            })
            .flatten()
            .ok_or_else(no_node)?;
            session.children_sent.insert(node_id);
            writer.event(
                "DOM.setChildNodes",
                json!({ "parentId": cdp_node_id(node_id), "nodes": nodes }),
            );
            Ok(json!({}))
        }

        "DOM.querySelector" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let context_id = node_id_param(params, "nodeId")?;
            let selector = params
                .get("selector")
                .and_then(|s| s.as_str())
                .ok_or_else(|| CdpError::invalid_params("Missing selector"))?
                .to_string();
            let children_sent = &mut session.children_sent;
            let found = with_doc(docs, doc_id, |doc| {
                doc.get_node(context_id).ok_or_else(no_node)?;
                let node_id = doc
                    .query_selector_in(context_id, &selector)
                    .map_err(|_| CdpError::server_error("DOM Error while querying"))?;
                if let Some(node_id) = node_id {
                    Session::emit_node_path(writer, doc, node_id, children_sent);
                }
                Ok(node_id)
            })
            .ok_or_else(no_document)??;
            match found {
                Some(node_id) => Ok(json!({ "nodeId": cdp_node_id(node_id) })),
                None => Ok(json!({ "nodeId": 0 })),
            }
        }

        "DOM.setAttributeValue" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = node_id_param(params, "nodeId")?;
            let name = str_param(params, "name")?;
            let value = str_param(params, "value")?;
            with_doc(docs, doc_id, |doc| {
                doc.get_node(node_id)
                    .filter(|node| node.element_data().is_some())
                    .ok_or_else(no_node)?;
                doc.mutate()
                    .set_attribute(node_id, attr_name(&name), &value);
                doc.shell_provider.request_redraw();
                Ok(())
            })
            .ok_or_else(no_document)??;
            writer.event(
                "DOM.attributeModified",
                json!({ "nodeId": cdp_node_id(node_id), "name": name, "value": value }),
            );
            Ok(json!({}))
        }

        "DOM.removeAttribute" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = node_id_param(params, "nodeId")?;
            let name = str_param(params, "name")?;
            with_doc(docs, doc_id, |doc| {
                doc.get_node(node_id)
                    .filter(|node| node.element_data().is_some())
                    .ok_or_else(no_node)?;
                doc.mutate().clear_attribute(node_id, attr_name(&name));
                doc.shell_provider.request_redraw();
                Ok(())
            })
            .ok_or_else(no_document)??;
            writer.event(
                "DOM.attributeRemoved",
                json!({ "nodeId": cdp_node_id(node_id), "name": name }),
            );
            Ok(json!({}))
        }

        // Sent by the Elements panel when an attribute is edited inline:
        // `text` is raw attribute markup (e.g. `id="foo" class="bar"`)
        // that replaces the attribute originally named `name`
        "DOM.setAttributesAsText" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = node_id_param(params, "nodeId")?;
            let text = str_param(params, "text")?;
            let replaced_name = params.get("name").and_then(|n| n.as_str());
            let new_attrs = parse_attributes_text(&text);
            let removed = replaced_name
                .filter(|name| !new_attrs.iter().any(|(new_name, _)| new_name == name))
                .map(|name| name.to_string());
            with_doc(docs, doc_id, |doc| {
                doc.get_node(node_id)
                    .filter(|node| node.element_data().is_some())
                    .ok_or_else(no_node)?;
                let mut mutr = doc.mutate();
                if let Some(name) = &removed {
                    mutr.clear_attribute(node_id, attr_name(name));
                }
                for (name, value) in &new_attrs {
                    mutr.set_attribute(node_id, attr_name(name), value);
                }
                drop(mutr);
                doc.shell_provider.request_redraw();
                Ok(())
            })
            .ok_or_else(no_document)??;
            if let Some(name) = &removed {
                writer.event(
                    "DOM.attributeRemoved",
                    json!({ "nodeId": cdp_node_id(node_id), "name": name }),
                );
            }
            for (name, value) in &new_attrs {
                writer.event(
                    "DOM.attributeModified",
                    json!({ "nodeId": cdp_node_id(node_id), "name": name, "value": value }),
                );
            }
            Ok(json!({}))
        }

        "DOM.setNodeValue" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = node_id_param(params, "nodeId")?;
            let value = str_param(params, "value")?;
            with_doc(docs, doc_id, |doc| {
                let node = doc.get_node_mut(node_id).ok_or_else(no_node)?;
                match &mut node.data {
                    NodeData::Text(_) => {}
                    NodeData::Comment { contents } => {
                        *contents = value.clone();
                        return Ok(());
                    }
                    _ => {
                        return Err(CdpError::server_error("Node is not a character data node"));
                    }
                }
                doc.mutate().set_node_text(node_id, &value);
                doc.shell_provider.request_redraw();
                Ok(())
            })
            .ok_or_else(no_document)??;
            writer.event(
                "DOM.characterDataModified",
                json!({ "nodeId": cdp_node_id(node_id), "characterData": value }),
            );
            Ok(json!({}))
        }

        "DOM.pushNodesByBackendIdsToFrontend" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let backend_ids = params
                .get("backendNodeIds")
                .and_then(|ids| ids.as_array())
                .ok_or_else(|| CdpError::invalid_params("Missing backendNodeIds"))?
                .clone();
            let children_sent = &mut session.children_sent;
            let node_ids = with_doc(docs, doc_id, |doc| {
                backend_ids
                    .iter()
                    .map(|id| {
                        let node_id = id.as_u64().and_then(blitz_node_id);
                        match node_id.and_then(|id| doc.get_node(id).map(|_| id)) {
                            Some(node_id) => {
                                Session::emit_node_path(writer, doc, node_id, children_sent);
                                cdp_node_id(node_id)
                            }
                            None => 0,
                        }
                    })
                    .collect::<Vec<u64>>()
            })
            .ok_or_else(no_document)?;
            Ok(json!({ "nodeIds": node_ids }))
        }

        "DOM.getBoxModel" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = any_node_id_param(params)?;
            let model = with_doc(docs, doc_id, |doc| crate::dom::box_model_json(doc, node_id))
                .flatten()
                .ok_or_else(|| CdpError::server_error("Could not compute box model."))?;
            Ok(json!({ "model": model }))
        }

        "DOM.getOuterHTML" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = any_node_id_param(params)?;
            let html = with_doc(docs, doc_id, |doc| crate::dom::outer_html(doc, node_id))
                .flatten()
                .ok_or_else(no_node)?;
            Ok(json!({ "outerHTML": html }))
        }

        "DOM.resolveNode" => Err(CdpError::server_error("No JavaScript runtime")),

        _ => Err(CdpError::method_not_found(method)),
    }
}

/// Parse raw attribute markup (e.g. `id="foo" data-x class='y'`) into
/// name/value pairs, as sent by `DOM.setAttributesAsText`
fn parse_attributes_text(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let mut name = String::new();
        while let Some(c) = chars.next_if(|&c| !c.is_whitespace() && c != '=') {
            name.push(c);
        }
        if name.is_empty() {
            break;
        }
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let mut value = String::new();
        if chars.next_if_eq(&'=').is_some() {
            while chars.next_if(|c| c.is_whitespace()).is_some() {}
            match chars.peek() {
                Some(&quote @ ('"' | '\'')) => {
                    chars.next();
                    while let Some(c) = chars.next_if(|&c| c != quote) {
                        value.push(c);
                    }
                    chars.next();
                }
                _ => {
                    while let Some(c) = chars.next_if(|&c| !c.is_whitespace()) {
                        value.push(c);
                    }
                }
            }
        }
        out.push((name, value));
    }
    out
}
