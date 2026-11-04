//! Handlers for the `Overlay` domain: node highlighting and inspect mode
//! (the element picker), including the picker's input-event handling

use blitz_dom::BaseDocument;
use blitz_traits::node_id::NodeId;
use serde_json::json;

use super::{CdpError, Session, any_node_id_param, cdp_node_id, no_document, with_doc};
use crate::{CdpCommand, DocumentProvider, JsonValue, MessageWriter, PickerEvent};

pub(super) fn dispatch(
    session: &mut Session,
    _writer: &mut MessageWriter,
    docs: &mut dyn DocumentProvider,
    command: &CdpCommand,
) -> Result<JsonValue, CdpError> {
    let method = command.method.as_str();
    let params = &command.params;
    match method {
        "Overlay.highlightNode" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let node_id = any_node_id_param(params)?;
            let highlighting = &mut session.highlighting;
            with_doc(docs, doc_id, |doc| {
                let highlight_id = nearest_element_ancestor(doc, node_id);
                *highlighting = highlight_id.is_some();
                doc.devtools_mut().highlight_node = highlight_id;
                doc.shell_provider.request_redraw();
            });
            Ok(json!({}))
        }

        "Overlay.hideHighlight" | "DOM.hideHighlight" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            session.highlighting = false;
            with_doc(docs, doc_id, |doc| {
                doc.devtools_mut().highlight_node = None;
                doc.shell_provider.request_redraw();
            });
            Ok(json!({}))
        }

        "Overlay.setInspectMode" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let mode = params
                .get("mode")
                .and_then(|m| m.as_str())
                .ok_or_else(|| CdpError::invalid_params("Missing mode"))?;
            let picking = mode == "searchForNode";
            session.picking = picking;
            session.last_picker_node = None;
            if !picking {
                session.highlighting = false;
            }
            with_doc(docs, doc_id, |doc| {
                doc.devtools_mut().element_picker = picking;
                if !picking {
                    doc.devtools_mut().highlight_node = None;
                }
                doc.shell_provider.request_redraw();
            });
            Ok(json!({}))
        }

        _ => Err(CdpError::method_not_found(method)),
    }
}

impl Session {
    /// Resolve the DOM node to report for a picker event at the given
    /// viewport position: the topmost hit node, mapped to the nearest
    /// ancestor that is shown in the devtools tree (non-anonymous and not a
    /// whitespace-only text node)
    fn picker_target(doc: &BaseDocument, x: f32, y: f32) -> Option<NodeId> {
        let hit = doc.hit(x, y)?;
        let mut node_id = doc.nearest_non_anonymous_ancestor(hit.node_id)?;
        while let Some(node) = doc.get_node(node_id) {
            if !node.is_whitespace_node() {
                return Some(node_id);
            }
            node_id = node.parent?;
        }
        None
    }

    /// React to an element-picker input event reported by the embedder,
    /// emitting the corresponding `Overlay` event to the client
    pub(crate) fn handle_picker_event(
        &mut self,
        writer: &mut MessageWriter,
        docs: &mut dyn DocumentProvider,
        event: &PickerEvent,
    ) {
        if !self.picking {
            return;
        }
        let event_doc_id = match *event {
            PickerEvent::Hovered { doc_id, .. }
            | PickerEvent::Picked { doc_id, .. }
            | PickerEvent::Canceled { doc_id } => doc_id,
        };
        if self.doc_id(docs) != Some(event_doc_id) {
            return;
        }

        match *event {
            PickerEvent::Hovered { x, y, .. } => {
                self.highlighting = true;
                let last_picker_node = &mut self.last_picker_node;
                let children_sent = &mut self.children_sent;
                let hovered = with_doc(docs, event_doc_id, |doc| {
                    let node_id = Self::picker_target(doc, x, y)?;
                    if *last_picker_node == Some(node_id) {
                        return None;
                    }
                    *last_picker_node = Some(node_id);
                    doc.devtools_mut().highlight_node = Some(node_id);
                    doc.shell_provider.request_redraw();
                    // The frontend reveals the hovered node in the Elements
                    // tree in realtime, which requires it to know the node:
                    // send its ancestor path first
                    Self::emit_node_path(writer, doc, node_id, children_sent);
                    Some(node_id)
                })
                .flatten();
                if let Some(node_id) = hovered {
                    writer.event(
                        "Overlay.nodeHighlightRequested",
                        json!({ "nodeId": cdp_node_id(node_id) }),
                    );
                }
            }
            PickerEvent::Picked { x, y, .. } => {
                self.picking = false;
                self.highlighting = false;
                self.last_picker_node = None;
                let picked = with_doc(docs, event_doc_id, |doc| {
                    let node_id = Self::picker_target(doc, x, y);
                    doc.devtools_mut().element_picker = false;
                    doc.devtools_mut().highlight_node = None;
                    doc.shell_provider.request_redraw();
                    node_id
                })
                .flatten();
                if let Some(node_id) = picked {
                    writer.event(
                        "Overlay.inspectNodeRequested",
                        json!({ "backendNodeId": cdp_node_id(node_id) }),
                    );
                }
            }
            PickerEvent::Canceled { .. } => {
                self.picking = false;
                self.highlighting = false;
                self.last_picker_node = None;
                with_doc(docs, event_doc_id, |doc| {
                    doc.devtools_mut().element_picker = false;
                    doc.devtools_mut().highlight_node = None;
                    doc.shell_provider.request_redraw();
                });
                writer.event("Overlay.inspectModeCanceled", json!({}));
            }
        }
    }

    /// Clean up any state this session holds on its document (inspect mode,
    /// node highlight), called when the client connection closes
    pub(crate) fn close(&mut self, docs: &mut dyn DocumentProvider) {
        if !self.picking && !self.highlighting {
            return;
        }
        self.picking = false;
        self.highlighting = false;
        self.last_picker_node = None;
        let Some(doc_id) = self.doc_id(docs) else {
            return;
        };
        with_doc(docs, doc_id, |doc| {
            doc.devtools_mut().element_picker = false;
            doc.devtools_mut().highlight_node = None;
            doc.shell_provider.request_redraw();
        });
    }
}

/// Text and comment nodes don't carry a layout of their own: find the
/// nearest element ancestor (starting from the node itself) to highlight
fn nearest_element_ancestor(doc: &BaseDocument, node_id: NodeId) -> Option<NodeId> {
    let mut current = Some(node_id);
    while let Some(id) = current {
        let node = doc.get_node(id)?;
        if node.element_data().is_some() {
            return Some(id);
        }
        current = node.parent;
    }
    None
}
