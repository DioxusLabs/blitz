use std::collections::HashSet;

use blitz_dom::BaseDocument;
use blitz_traits::node_id::NodeId;
use serde_json::json;

use crate::{CdpCommand, DocumentProvider, JsonValue, MessageWriter, PickerEvent};

/// JSON-RPC error codes used in CDP error replies
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const SERVER_ERROR: i64 = -32000;

pub(crate) struct CdpError {
    code: i64,
    message: String,
}

impl CdpError {
    fn method_not_found(method: &str) -> Self {
        CdpError {
            code: METHOD_NOT_FOUND,
            message: format!("'{method}' wasn't found"),
        }
    }
    fn invalid_params(message: &str) -> Self {
        CdpError {
            code: INVALID_PARAMS,
            message: message.to_string(),
        }
    }
    fn server_error(message: &str) -> Self {
        CdpError {
            code: SERVER_ERROR,
            message: message.to_string(),
        }
    }
}

/// Convert a Blitz `NodeId` to a CDP node id (CDP node ids must be non-zero,
/// and Blitz node ids start at 0)
pub(crate) fn cdp_node_id(node_id: NodeId) -> u64 {
    node_id.as_u64() + 1
}

/// Convert a CDP node id back to a Blitz `NodeId`
pub(crate) fn blitz_node_id(cdp_id: u64) -> Option<NodeId> {
    cdp_id.checked_sub(1).map(NodeId::from_u64)
}

/// Domain prefixes for which all commands are stubbed with an empty reply:
/// these domains are irrelevant to the Elements panel but the frontend
/// enables/configures them at startup
const STUB_DOMAIN_PREFIXES: &[&str] = &[
    "Network.",
    "Log.",
    "Emulation.",
    "Audits.",
    "Profiler.",
    "Debugger.",
    "Security.",
    "ServiceWorker.",
    "Storage.",
    "Tracing.",
    "Fetch.",
    "Autofill.",
    "Preload.",
    "BackgroundService.",
    "CacheStorage.",
    "IndexedDB.",
    "DOMStorage.",
    "Database.",
    "DOMDebugger.",
    "EventBreakpoints.",
    "Media.",
    "Animation.",
    "Accessibility.",
    "LayerTree.",
    "Memory.",
    "Performance.",
    "Input.",
    "IO.",
];

/// Individual commands stubbed with an empty reply
const STUB_METHODS: &[&str] = &[
    "Target.setAutoAttach",
    "Target.setDiscoverTargets",
    "Target.setRemoteLocations",
    "Page.setLifecycleEventsEnabled",
    "Page.setAdBlockingEnabled",
    "Page.stopScreencast",
    "Runtime.releaseObjectGroup",
    "Runtime.runIfWaitingForDebugger",
    "Runtime.discardConsoleEntries",
    "Runtime.addBinding",
    "DOM.setInspectedNode",
    "DOM.markUndoableState",
    "DOM.undo",
    "DOM.redo",
    "DOM.discardSearchResults",
    "DOM.setNodeStackTracesEnabled",
    "CSS.getBackgroundColors",
    "CSS.trackComputedStyleUpdates",
    "CSS.trackComputedStyleUpdatesForNode",
    "CSS.takeComputedStyleUpdates",
    "CSS.startRuleUsageTracking",
    "Overlay.setShowViewportSizeOnResize",
    "Overlay.setShowGridOverlays",
    "Overlay.setShowFlexOverlays",
    "Overlay.setShowScrollSnapOverlays",
    "Overlay.setShowContainerQueryOverlays",
    "Overlay.setShowIsolatedElements",
    "Overlay.setShowHinge",
    "Overlay.setShowAdHighlights",
    "Overlay.setShowLayoutShiftRegions",
    "Overlay.setShowPaintRects",
    "Overlay.setShowDebugBorders",
    "Overlay.setShowFPSCounter",
    "Overlay.setShowScrollBottleneckRects",
    "Overlay.setShowWebVitals",
    "Overlay.setPausedInDebuggerMessage",
    "Overlay.highlightRect",
    "Overlay.highlightQuad",
    "Overlay.highlightFrame",
];

/// A single CDP client session, attached to one Blitz document
pub(crate) struct Session {
    /// The document id requested via the WebSocket path
    doc_id_hint: Option<usize>,
    /// Whether inspect mode (the element picker) is currently active
    pub(crate) picking: bool,
    /// Whether this session currently has a node highlighted on the document
    highlighting: bool,
    /// The node last reported via an `Overlay.nodeHighlightRequested` event
    /// (used to avoid re-sending events while hovering the same node)
    last_picker_node: Option<NodeId>,
    /// Nodes whose children have already been sent to the frontend (inline
    /// in a `DOM.getDocument` reply or via `DOM.setChildNodes`). Resending
    /// children replaces the frontend's node objects, breaking tree state
    /// such as revealing a picked node, so they are only sent once.
    children_sent: HashSet<NodeId>,
}

impl Session {
    pub(crate) fn new(doc_id_hint: Option<usize>) -> Self {
        Session {
            doc_id_hint,
            picking: false,
            highlighting: false,
            last_picker_node: None,
            children_sent: HashSet::new(),
        }
    }

    /// The id of the document this session is inspecting: the one requested
    /// via the WebSocket path (fixed for the session's lifetime — `None` if
    /// it has closed), or the first document if none was requested
    fn doc_id(&self, docs: &dyn DocumentProvider) -> Option<usize> {
        let ids = docs.document_ids();
        match self.doc_id_hint {
            Some(id) => ids.contains(&id).then_some(id),
            None => ids.first().copied(),
        }
    }

    pub(crate) fn handle_command(
        &mut self,
        writer: &mut MessageWriter,
        docs: &mut dyn DocumentProvider,
        command: CdpCommand,
    ) {
        match self.dispatch(writer, docs, &command) {
            Ok(result) => writer.reply(&command, result),
            Err(err) => writer.reply_err(&command, err.code, &err.message),
        }
    }

    fn dispatch(
        &mut self,
        writer: &mut MessageWriter,
        docs: &mut dyn DocumentProvider,
        command: &CdpCommand,
    ) -> Result<JsonValue, CdpError> {
        let method = command.method.as_str();
        let params = &command.params;

        // Generic stubs: domain enable/disable commands and commands from
        // domains irrelevant to the Elements panel
        if method.ends_with(".enable")
            || method.ends_with(".disable")
            || STUB_METHODS.contains(&method)
            || STUB_DOMAIN_PREFIXES
                .iter()
                .any(|prefix| method.starts_with(prefix))
        {
            return Ok(json!({}));
        }

        match method {
            "Browser.getVersion" => Ok(json!({
                "protocolVersion": "1.3",
                "product": "Blitz",
                "revision": "",
                "userAgent": "Blitz",
                "jsVersion": "",
            })),

            "Runtime.evaluate" | "Runtime.callFunctionOn" => {
                Ok(json!({ "result": { "type": "undefined" } }))
            }

            "Page.getResourceTree" | "Page.getFrameTree" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let url =
                    with_doc(docs, doc_id, |doc| doc.url().to_string()).ok_or_else(no_document)?;
                let frame = json!({
                    "id": format!("frame-{doc_id}"),
                    "loaderId": format!("loader-{doc_id}"),
                    "url": url,
                    "domainAndRegistry": "",
                    "securityOrigin": "",
                    "mimeType": "text/html",
                    "secureContextType": "Secure",
                    "crossOriginIsolatedContextType": "NotIsolated",
                    "gatedAPIFeatures": [],
                });
                match method {
                    "Page.getResourceTree" => {
                        Ok(json!({ "frameTree": { "frame": frame, "resources": [] } }))
                    }
                    _ => Ok(json!({ "frameTree": { "frame": frame } })),
                }
            }
            "Page.getNavigationHistory" => Ok(json!({ "currentIndex": 0, "entries": [] })),

            "DOM.getDocument" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let depth = params.get("depth").and_then(|d| d.as_i64()).unwrap_or(1);
                self.children_sent.clear();
                let children_sent = &mut self.children_sent;
                let root = with_doc(docs, doc_id, |doc| {
                    crate::dom::node_json(doc, doc.root_node().id, depth, children_sent)
                })
                .flatten()
                .ok_or_else(no_document)?;
                Ok(json!({ "root": root }))
            }

            "DOM.requestChildNodes" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let node_id = node_id_param(params, "nodeId")?;
                let depth = params.get("depth").and_then(|d| d.as_i64()).unwrap_or(1);
                if self.children_sent.contains(&node_id) {
                    // The frontend already knows this node's children:
                    // resending them would replace its node objects and
                    // break tree state (see `children_sent`)
                    return Ok(json!({}));
                }
                let children_sent = &mut self.children_sent;
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
                self.children_sent.insert(node_id);
                writer.event(
                    "DOM.setChildNodes",
                    json!({ "parentId": cdp_node_id(node_id), "nodes": nodes }),
                );
                Ok(json!({}))
            }

            "DOM.querySelector" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let selector = params
                    .get("selector")
                    .and_then(|s| s.as_str())
                    .ok_or_else(|| CdpError::invalid_params("Missing selector"))?
                    .to_string();
                let children_sent = &mut self.children_sent;
                let found = with_doc(docs, doc_id, |doc| {
                    let node_id = doc.query_selector(&selector).ok().flatten()?;
                    Self::emit_node_path(writer, doc, node_id, children_sent);
                    Some(node_id)
                })
                .ok_or_else(no_document)?;
                match found {
                    Some(node_id) => Ok(json!({ "nodeId": cdp_node_id(node_id) })),
                    None => Ok(json!({ "nodeId": 0 })),
                }
            }

            "DOM.pushNodesByBackendIdsToFrontend" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let backend_ids = params
                    .get("backendNodeIds")
                    .and_then(|ids| ids.as_array())
                    .ok_or_else(|| CdpError::invalid_params("Missing backendNodeIds"))?
                    .clone();
                let children_sent = &mut self.children_sent;
                let node_ids = with_doc(docs, doc_id, |doc| {
                    backend_ids
                        .iter()
                        .map(|id| {
                            let node_id = id.as_u64().and_then(blitz_node_id);
                            match node_id.and_then(|id| doc.get_node(id).map(|_| id)) {
                                Some(node_id) => {
                                    Self::emit_node_path(writer, doc, node_id, children_sent);
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
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let node_id = any_node_id_param(params)?;
                let model = with_doc(docs, doc_id, |doc| crate::dom::box_model_json(doc, node_id))
                    .flatten()
                    .ok_or_else(|| CdpError::server_error("Could not compute box model."))?;
                Ok(json!({ "model": model }))
            }

            "DOM.getOuterHTML" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let node_id = any_node_id_param(params)?;
                let html = with_doc(docs, doc_id, |doc| crate::dom::outer_html(doc, node_id))
                    .flatten()
                    .ok_or_else(no_node)?;
                Ok(json!({ "outerHTML": html }))
            }

            "DOM.resolveNode" => Err(CdpError::server_error("No JavaScript runtime")),

            "CSS.getMatchedStylesForNode" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let node_id = node_id_param(params, "nodeId")?;
                with_doc(docs, doc_id, |doc| {
                    crate::css::matched_styles_json(doc, node_id)
                })
                .flatten()
                .ok_or_else(no_node)
            }

            "CSS.getComputedStyleForNode" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let node_id = node_id_param(params, "nodeId")?;
                let computed = with_doc(docs, doc_id, |doc| {
                    crate::css::computed_style_json(doc, node_id)
                })
                .flatten()
                .ok_or_else(no_node)?;
                Ok(json!({ "computedStyle": computed }))
            }

            "CSS.getInlineStylesForNode" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let node_id = node_id_param(params, "nodeId")?;
                let inline = with_doc(docs, doc_id, |doc| {
                    crate::css::inline_style_json(doc, node_id)
                })
                .ok_or_else(no_node)?;
                Ok(json!({ "inlineStyle": inline, "attributesStyle": null }))
            }

            "CSS.getPlatformFontsForNode" => Ok(json!({ "fonts": [] })),

            "Overlay.highlightNode" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let node_id = any_node_id_param(params)?;
                let highlighting = &mut self.highlighting;
                with_doc(docs, doc_id, |doc| {
                    let highlight_id = nearest_element_ancestor(doc, node_id);
                    *highlighting = highlight_id.is_some();
                    doc.devtools_mut().highlight_node = highlight_id;
                    doc.shell_provider.request_redraw();
                });
                Ok(json!({}))
            }

            "Overlay.hideHighlight" | "DOM.hideHighlight" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                self.highlighting = false;
                with_doc(docs, doc_id, |doc| {
                    doc.devtools_mut().highlight_node = None;
                    doc.shell_provider.request_redraw();
                });
                Ok(json!({}))
            }

            // The screencast pane (shown by the chrome://inspect frontend)
            // is not supported: report it as not visible so its blank pane
            // shows an explanatory message rather than appearing broken.
            // Note that while the screencast is toggled on, the frontend
            // routes element picking and node highlighting to the screencast
            // view instead of the Overlay domain; toggling it off restores
            // protocol-based picking on the Blitz window itself.
            "Page.startScreencast" => {
                writer.event(
                    "Page.screencastVisibilityChanged",
                    json!({ "visible": false }),
                );
                Ok(json!({}))
            }

            "Overlay.setInspectMode" => {
                let doc_id = self.doc_id(docs).ok_or_else(no_document)?;
                let mode = params
                    .get("mode")
                    .and_then(|m| m.as_str())
                    .ok_or_else(|| CdpError::invalid_params("Missing mode"))?;
                let picking = mode == "searchForNode";
                self.picking = picking;
                self.last_picker_node = None;
                if !picking {
                    self.highlighting = false;
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

    /// Emit `DOM.setChildNodes` events describing the path from the document
    /// root down to (and including the siblings of) the given node, so that
    /// the frontend can connect the node to its existing tree. Ancestors
    /// whose children were already sent are skipped: resending them would
    /// replace the frontend's node objects and detach its current tree
    /// selection/expansion state.
    fn emit_node_path(
        writer: &mut MessageWriter,
        doc: &BaseDocument,
        node_id: NodeId,
        children_sent: &mut HashSet<NodeId>,
    ) {
        // Collect the ancestor chain (excluding the node itself), root first
        let mut chain = Vec::new();
        let mut current = doc.get_node(node_id).and_then(|node| node.parent);
        while let Some(ancestor_id) = current {
            chain.push(ancestor_id);
            current = doc.get_node(ancestor_id).and_then(|node| node.parent);
        }
        chain.reverse();

        for ancestor_id in chain {
            if children_sent.contains(&ancestor_id) {
                continue;
            }
            let Some(ancestor) = doc.get_node(ancestor_id) else {
                continue;
            };
            let nodes: Vec<JsonValue> = crate::dom::dom_children(doc, ancestor)
                .iter()
                .filter_map(|child| crate::dom::node_json(doc, child.id, 0, children_sent))
                .collect();
            children_sent.insert(ancestor_id);
            writer.event(
                "DOM.setChildNodes",
                json!({ "parentId": cdp_node_id(ancestor_id), "nodes": nodes }),
            );
        }
    }

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

fn no_document() -> CdpError {
    CdpError::server_error("No document")
}

fn no_node() -> CdpError {
    CdpError::server_error("Could not find node with given id")
}

/// Extract a Blitz `NodeId` from a CDP node id parameter
fn node_id_param(params: &JsonValue, key: &str) -> Result<NodeId, CdpError> {
    params
        .get(key)
        .and_then(|id| id.as_u64())
        .and_then(blitz_node_id)
        .ok_or_else(|| CdpError::invalid_params(&format!("Missing {key}")))
}

/// Extract a Blitz `NodeId` from either a `nodeId` or a `backendNodeId`
/// parameter (they use the same id space in this implementation)
fn any_node_id_param(params: &JsonValue) -> Result<NodeId, CdpError> {
    node_id_param(params, "nodeId").or_else(|_| node_id_param(params, "backendNodeId"))
}

/// Run a callback with access to the document with the given id, returning
/// `None` if no such document exists
pub(crate) fn with_doc<R>(
    docs: &mut dyn DocumentProvider,
    doc_id: usize,
    cb: impl FnOnce(&mut BaseDocument) -> R,
) -> Option<R> {
    let mut cb = Some(cb);
    let mut result = None;
    docs.with_document(doc_id, &mut |doc| {
        if let Some(cb) = cb.take() {
            result = Some(cb(doc));
        }
    });
    result
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
