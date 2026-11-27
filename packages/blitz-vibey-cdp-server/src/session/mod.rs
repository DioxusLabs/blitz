//! A CDP client session: per-connection protocol state and command
//! dispatch, with the command handlers split into sub-modules by domain

use std::collections::HashSet;

use blitz_dom::{BaseDocument, LocalName, QualName, ns};
use blitz_traits::node_id::NodeId;
use serde_json::json;

use crate::style_sheets::StyleSheets;
use crate::{CdpCommand, DocumentProvider, JsonValue, MessageWriter, TargetId};

mod css;
mod dom;
mod mutations;
mod overlay;
mod page;

/// JSON-RPC error codes used in CDP error replies
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const SERVER_ERROR: i64 = -32000;

pub(crate) struct CdpError {
    pub(crate) code: i64,
    pub(crate) message: String,
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

/// A single CDP client session, attached to one Blitz target
pub(crate) struct Session {
    /// The target requested via the WebSocket path
    target: Option<TargetId>,
    /// The id of the document the target showed when last checked
    bound_doc_id: Option<usize>,
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
    /// The style sheets reported to the frontend, while the `CSS` domain
    /// is enabled
    pub(crate) style_sheets: Option<StyleSheets>,
}

impl Session {
    pub(crate) fn new(target: Option<TargetId>) -> Self {
        Session {
            target,
            bound_doc_id: None,
            picking: false,
            highlighting: false,
            last_picker_node: None,
            children_sent: HashSet::new(),
            style_sheets: None,
        }
    }

    /// The id of the document this session is inspecting: the one the
    /// requested target showed when last bound (`None` if it has closed),
    /// or the first document if no target was requested
    pub(crate) fn doc_id(&self, docs: &dyn DocumentProvider) -> Option<usize> {
        let ids = docs.document_ids();
        match self.target {
            Some(_) => self.bound_doc_id.filter(|id| ids.contains(id)),
            None => ids.first().copied(),
        }
    }

    /// Bind the session to the document its target currently shows. If the
    /// target has navigated to a new document since it was last bound, the
    /// frontend's tree is discarded with a `DOM.documentUpdated` event (as
    /// Chrome does), after which it requests the new document.
    pub(crate) fn rebind(&mut self, writer: &mut MessageWriter, docs: &mut dyn DocumentProvider) {
        let Some(target) = self.target else {
            return;
        };
        let doc_id = target.resolve(docs);
        if doc_id == self.bound_doc_id {
            return;
        }
        tracing::debug!(
            "CDP: target {target} now shows document {doc_id:?} (was {:?})",
            self.bound_doc_id
        );
        let was_bound = self.bound_doc_id.is_some();
        self.bound_doc_id = doc_id;
        self.children_sent.clear();
        // The frontend drops its style sheets with the old document
        if self.style_sheets.is_some() {
            self.style_sheets = Some(StyleSheets::default());
        }
        self.highlighting = false;
        self.last_picker_node = None;
        let Some(doc_id) = doc_id else {
            return;
        };
        // Inspect mode stays active across navigations
        if self.picking {
            with_doc(docs, doc_id, |doc| doc.devtools_mut().element_picker = true);
        }
        if was_bound {
            writer.event("DOM.documentUpdated", json!({}));
        }
    }

    /// Report style sheets added to or removed from the document, if the
    /// `CSS` domain is enabled
    pub(crate) fn sync_style_sheets(
        &mut self,
        writer: &mut MessageWriter,
        docs: &mut dyn DocumentProvider,
    ) {
        let doc_id = self.doc_id(docs);
        let (Some(sheets), Some(doc_id)) = (&mut self.style_sheets, doc_id) else {
            return;
        };
        with_doc(docs, doc_id, |doc| sheets.sync(doc, doc_id, writer));
    }

    pub(crate) fn dispatch(
        &mut self,
        writer: &mut MessageWriter,
        docs: &mut dyn DocumentProvider,
        command: &CdpCommand,
    ) -> Result<JsonValue, CdpError> {
        let method = command.method.as_str();

        if method.starts_with("CSS.") && !STUB_METHODS.contains(&method) {
            return css::dispatch(self, writer, docs, command);
        }

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
            m if m.starts_with("Overlay.") || m == "DOM.hideHighlight" => {
                overlay::dispatch(self, writer, docs, command)
            }
            m if m.starts_with("DOM.") => dom::dispatch(self, writer, docs, command),
            _ => page::dispatch(self, writer, docs, command),
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
}

/// Whether the frontend has a node object for the given node: the document
/// itself, or any child of a node whose children have been sent
fn is_known_to_frontend(
    doc: &BaseDocument,
    node_id: NodeId,
    children_sent: &HashSet<NodeId>,
) -> bool {
    node_id == doc.root_node().id
        || doc
            .get_node(node_id)
            .and_then(|node| node.parent)
            .is_some_and(|parent_id| children_sent.contains(&parent_id))
}

/// Forget that the children of a node (and its descendants) were sent:
/// the frontend discards its node objects for a removed subtree, so they
/// must be sent afresh if the subtree is reinserted
fn forget_subtree(doc: &BaseDocument, node_id: NodeId, children_sent: &mut HashSet<NodeId>) {
    children_sent.remove(&node_id);
    if let Some(node) = doc.get_node(node_id) {
        for child_id in &node.children {
            forget_subtree(doc, *child_id, children_sent);
        }
    }
}

/// Tell the frontend that `parent_id`'s child list changed: emit
/// `DOM.childNodeCountUpdated` if it knows the parent but not its children
fn emit_child_count_updated(
    writer: &mut MessageWriter,
    doc: &BaseDocument,
    parent_id: NodeId,
    children_sent: &HashSet<NodeId>,
) {
    if children_sent.contains(&parent_id) || !is_known_to_frontend(doc, parent_id, children_sent) {
        return;
    }
    let Some(parent) = doc.get_node(parent_id) else {
        return;
    };
    writer.event(
        "DOM.childNodeCountUpdated",
        json!({
            "nodeId": cdp_node_id(parent_id),
            "childNodeCount": crate::dom::dom_children(doc, parent).len(),
        }),
    );
}

/// Check that a node can be removed or replaced: the document and its root
/// element cannot be
fn check_removable(doc: &BaseDocument, node_id: NodeId) -> Result<NodeId, CdpError> {
    let node = doc.get_node(node_id).ok_or_else(no_node)?;
    let parent_id = node
        .parent
        .ok_or_else(|| CdpError::server_error("Node has no parent"))?;
    if parent_id == doc.root_node().id && node.is_element() {
        return Err(CdpError::server_error("Cannot modify the document element"));
    }
    Ok(parent_id)
}

fn no_document() -> CdpError {
    CdpError::server_error("No document")
}

fn no_node() -> CdpError {
    CdpError::server_error("Could not find node with given id")
}

fn no_style_sheet() -> CdpError {
    CdpError::server_error("No style sheet with given id found")
}

/// Extract a string parameter
fn str_param(params: &JsonValue, key: &str) -> Result<String, CdpError> {
    params
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| CdpError::invalid_params(&format!("Missing {key}")))
}

/// The qualified name of an (HTML, un-namespaced) attribute
fn attr_name(name: &str) -> QualName {
    QualName {
        prefix: None,
        ns: ns!(),
        local: LocalName::from(name),
    }
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
