//! Shared state accessible from both the Rust side (`ScriptDocument`) and the
//! JavaScript side (native functions registered with the Boa `Context`).

use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::{BaseDocument, NodeId};
use boa_engine::{Finalize, JsData, Trace};
use boa_gc::force_collect;

use crate::clock::ScriptClock;
use crate::node_wrappers::{NodeWrappers, cleanup_detached_subtree, has_live_descendant};
use crate::timers::TimerQueue;

/// The document's `readyState`
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ReadyState {
    #[default]
    Loading,
    Interactive,
    Complete,
}

impl ReadyState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Interactive => "interactive",
            Self::Complete => "complete",
        }
    }
}

/// State owned by the script runtime but shared (via `Rc`) with the native
/// functions exposed to JavaScript.
///
/// Note: this struct stores Boa GC handles (`JsObject`s) in ordinary Rust
/// collections. That is sound because Boa GC handles held outside of the GC
/// heap act as roots (they keep their referents alive).
#[derive(Default)]
pub(crate) struct RuntimeState {
    /// Cache of JS wrapper objects, keyed by node id (see [`NodeWrappers`]).
    pub node_wrappers: NodeWrappers,
    /// Pending timers (`setTimeout`/`setInterval`/`requestAnimationFrame`)
    pub timers: TimerQueue,
    /// The clock driving timer deadlines and `Date`
    pub clock: ScriptClock,
    /// Messages sent from JavaScript to the embedder via the
    /// `__blitz_send_message` native function. Drained with
    /// [`ScriptDocument::take_messages`](crate::ScriptDocument::take_messages).
    pub outbound_messages: Vec<String>,
    /// The value exposed as `document.readyState`
    pub ready_state: ReadyState,
    /// Uncaught JavaScript errors (from script loading/evaluation, event
    /// listeners, timer callbacks and promise jobs). Drained with
    /// [`ScriptDocument::take_js_errors`](crate::ScriptDocument::take_js_errors).
    pub uncaught_errors: Vec<String>,
}

/// Maximum number of errors stored in [`RuntimeState::uncaught_errors`] between
/// drains, so that memory use is bounded for embedders which never drain them
const MAX_STORED_ERRORS: usize = 256;

impl RuntimeState {
    /// Record an error for the embedder to collect via
    /// [`ScriptDocument::take_js_errors`](crate::ScriptDocument::take_js_errors).
    /// Errors beyond [`MAX_STORED_ERRORS`] are dropped (with a marker) until the
    /// stored errors are drained.
    pub fn record_error(&mut self, message: String) {
        use std::cmp::Ordering;
        match self.uncaught_errors.len().cmp(&MAX_STORED_ERRORS) {
            Ordering::Less => self.uncaught_errors.push(message),
            Ordering::Equal => self
                .uncaught_errors
                .push("(further errors suppressed)".to_string()),
            Ordering::Greater => {}
        }
    }
}

/// Cloneable handle to the document and the runtime state. This is stored as
/// host-defined data on the Boa [`Context`](boa_engine::Context) so that native
/// functions can access the DOM.
#[derive(Clone, Trace, Finalize, JsData)]
pub(crate) struct DomCtx {
    #[unsafe_ignore_trace]
    pub doc: Rc<RefCell<BaseDocument>>,
    #[unsafe_ignore_trace]
    pub state: Rc<RefCell<RuntimeState>>,
}

impl DomCtx {
    pub fn new(doc: Rc<RefCell<BaseDocument>>) -> Self {
        Self {
            doc,
            state: Rc::new(RefCell::new(RuntimeState::default())),
        }
    }

    // ── Reference switching ──────────────────────────────────────────

    /// Check if a node is in the document using the blitz internal flag.
    pub(crate) fn is_in_document(&self, node_id: NodeId) -> bool {
        self.doc
            .borrow()
            .get_node(node_id)
            .is_some_and(|node| node.flags.is_in_document())
    }

    /// Recursively switch all cached wrappers in a subtree to strong refs.
    ///
    /// **Caller must ensure `node_id` is in the document.**
    fn make_subtree_strong(&self, node_id: NodeId) {
        self.state.borrow_mut().node_wrappers.make_strong(node_id);
        let child_ids: Vec<NodeId> = self
            .doc
            .borrow()
            .get_node(node_id)
            .map(|node| node.children.to_vec())
            .unwrap_or_default();
        for child_id in child_ids {
            self.make_subtree_strong(child_id);
        }
    }

    /// Recursively switch all cached wrappers in a subtree to weak refs.
    fn make_subtree_weak(&self, node_id: NodeId) {
        self.state.borrow_mut().node_wrappers.make_weak(node_id);
        let child_ids: Vec<NodeId> = self
            .doc
            .borrow()
            .get_node(node_id)
            .map(|node| node.children.to_vec())
            .unwrap_or_default();
        for child_id in child_ids {
            self.make_subtree_weak(child_id);
        }
    }

    /// Switch a subtree to strong refs if the parent is in the document.
    pub(crate) fn make_in_document_subtree_strong(&self, parent_id: NodeId, child_id: NodeId) {
        if self.is_in_document(parent_id) {
            self.make_subtree_strong(child_id);
        }
    }

    /// Switch a subtree to weak refs if the node is in the document.
    /// If the node is already detached, no-op.
    ///
    /// **Must be called before `remove_node`**, while the node still has its
    /// parent chain so `is_in_document` can be evaluated.
    pub(crate) fn make_in_document_subtree_weak(&self, node_id: NodeId) {
        if self.is_in_document(node_id) {
            self.make_subtree_weak(node_id);
        }
    }

    /// Apply queued weak switches and reclaim nodes whose wrapper the GC
    /// collected.
    ///
    /// `WeakJsObject::new` allocates in the GC heap and can trigger a
    /// synchronous `Collector::collect`; running it while a `RuntimeState` or
    /// document borrow is alive would let the collector re-enter those cells
    /// and panic. The queued switches, the collection, and the reclamation
    /// therefore all run here, where neither cell is borrowed. Call from the
    /// embedder side after JS execution, never while holding either cell.
    pub(crate) fn flush_wrapper_switches(&self) {
        if !self.state.borrow().node_wrappers.has_pending_weak() {
            return;
        }
        self.state.borrow_mut().node_wrappers.flush_pending_weak();
        // Make the weakened wrappers reclaimable right now instead of waiting
        // for an allocation-driven collection, so the reclamation below sees
        // a stable state
        force_collect();
        let stale = self.state.borrow_mut().node_wrappers.take_stale();
        for node_id in stale {
            self.reclaim_detached_node(node_id);
        }
    }

    /// Reclaim the Rust-side storage of a node whose JS wrapper was collected.
    fn reclaim_detached_node(&self, node_id: NodeId) {
        let mut doc = self.doc.borrow_mut();
        let Some(node) = doc.get_node(node_id) else {
            return;
        };
        let is_detached = node.parent.is_none();

        let wrappers = &self.state.borrow().node_wrappers;
        if is_detached && !has_live_descendant(&doc, wrappers, node_id) {
            doc.mutate().remove_and_drop_node(node_id);
            return;
        }
        cleanup_detached_subtree(&mut doc, wrappers, node_id);
    }

    /// Collect, weaken, and detach all children of `node_id`.
    pub(crate) fn detach_children(&self, node_id: NodeId) {
        let children: Vec<NodeId> = {
            let doc = self.doc.borrow();
            doc.get_node(node_id)
                .map(|node| node.children.iter().copied().collect())
                .unwrap_or_default()
        };
        for child_id in &children {
            self.make_in_document_subtree_weak(*child_id);
        }
        let mut doc = self.doc.borrow_mut();
        let mut mutator = doc.mutate();
        for child_id in &children {
            mutator.remove_node(*child_id);
        }
    }
}
