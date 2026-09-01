//! Switchable-reference cache of JS node wrapper objects, keyed by node id.
//!
//! DOM wrappers must be cached so that a given DOM node is always represented
//! by the *same* JS object: scripts rely on object identity (`===`) and on
//! expando properties persisting across accesses.
//!
//! Entries start strong so that event listeners registered on wrappers are
//! never lost, and can be switched to weak to hand a detached node's wrapper
//! back to the GC. When a weak wrapper is collected, the GC runs `NodeLayer`'s
//! finalizer, which removes the stale entry and reclaims the detached
//! subtree's Rust-side node storage (see [`cleanup_detached_subtree`]).

use std::collections::HashMap;

use blitz_dom::{BaseDocument, NodeId};
use boa_engine::object::JsObject;

use crate::switchable_ref::SwitchableRef;

/// Switchable-reference cache: `node_id -> SwitchableRef`.
#[derive(Default)]
pub(crate) struct NodeWrappers {
    entries: HashMap<NodeId, SwitchableRef>,
}

impl NodeWrappers {
    /// Try to retrieve a cached wrapper. Returns `None` if the cache has no
    /// entry for `node_id` or the reference is dead (wrapper collected, only
    /// possible in weak mode).
    pub(crate) fn get(&self, node_id: NodeId) -> Option<JsObject> {
        self.entries.get(&node_id)?.get()
    }

    /// Cache a freshly created wrapper (strong).
    pub(crate) fn insert(&mut self, node_id: NodeId, obj: JsObject) {
        self.entries.insert(node_id, SwitchableRef::new(obj));
    }

    /// Whether an entry exists for `node_id`, dead weak entries included.
    pub(crate) fn contains_key(&self, node_id: NodeId) -> bool {
        self.entries.contains_key(&node_id)
    }

    /// Switch a cache entry to weak. No-op if already weak or not in cache.
    pub(crate) fn make_weak(&mut self, node_id: NodeId) {
        if let Some(entry) = self.entries.get_mut(&node_id) {
            entry.make_weak();
        }
    }

    /// Switch a cache entry to strong. No-op if already strong. Returns
    /// `false` if the entry is missing or its wrapper was collected.
    pub(crate) fn make_strong(&mut self, node_id: NodeId) -> bool {
        self.entries
            .get_mut(&node_id)
            .is_some_and(|entry| entry.make_strong())
    }

    /// Remove a cache entry. Called when a weak wrapper has been collected
    /// (see `NodeLayer`'s `Finalize` impl).
    pub(crate) fn remove(&mut self, node_id: NodeId) {
        self.entries.remove(&node_id);
    }

    /// Whether the entry's wrapper is still alive. `false` if the cache has
    /// no entry for `node_id` or the wrapper was collected (only possible in
    /// weak mode).
    pub(crate) fn is_alive(&self, node_id: NodeId) -> bool {
        self.entries
            .get(&node_id)
            .is_some_and(|entry| entry.is_alive())
    }

    /// Remove all entries whose reference is dead (wrapper collected).
    #[allow(unused)]
    pub(crate) fn sweep(&mut self) {
        let stale: Vec<NodeId> = self
            .entries
            .iter()
            .filter_map(|(&id, entry)| (!entry.is_alive()).then_some(id))
            .collect();
        for id in stale {
            self.entries.remove(&id);
        }
    }

    /// Number of entries currently in the cache.
    #[allow(unused)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[allow(unused)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Recursively check whether any descendant of `node_id` has a live entry in
/// `wrappers`.
pub(crate) fn has_live_descendant(
    doc: &BaseDocument,
    wrappers: &NodeWrappers,
    node_id: NodeId,
) -> bool {
    let child_ids: Vec<NodeId> = doc
        .get_node(node_id)
        .map(|node| node.children.to_vec())
        .unwrap_or_default();
    for child_id in child_ids {
        if wrappers.is_alive(child_id) {
            return true;
        }
        if has_live_descendant(doc, wrappers, child_id) {
            return true;
        }
    }
    false
}

/// From a detached node, walk up to the topmost ancestor that still exists in
/// the slab, then drop that whole subtree if none of its wrappers are live.
pub(crate) fn cleanup_detached_subtree(
    doc: &mut BaseDocument,
    wrappers: &NodeWrappers,
    node_id: NodeId,
) {
    let mut top = node_id;
    while let Some(parent_id) = doc.get_node(top).and_then(|node| node.parent) {
        if doc.get_node(parent_id).is_none() {
            break;
        }
        top = parent_id;
    }
    if top == node_id {
        return;
    }
    if wrappers.is_alive(top) {
        return;
    }
    if has_live_descendant(doc, wrappers, top) {
        return;
    }
    doc.mutate().remove_and_drop_node(top);
}
