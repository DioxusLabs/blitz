//! Versioned storage for the nodes of the DOM tree.

use std::ops::{Index, IndexMut};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use blitz_traits::node_id::NodeId;
use slotmap::{Key as _, KeyData, SlotMap};

use crate::Node;

slotmap::new_key_type! {
    /// The internal [`slotmap`] key for node storage. Only used at the
    /// storage boundary: all public APIs use [`NodeId`].
    struct NodeKey;
}

#[inline(always)]
fn to_key(id: NodeId) -> NodeKey {
    NodeKey::from(KeyData::from_ffi(id.as_u64()))
}

#[inline(always)]
fn to_id(key: NodeKey) -> NodeId {
    NodeId::from_u64(key.data().as_ffi())
}

/// The versioned map in which the nodes of the DOM tree are stored, backed by
/// a [`slotmap::SlotMap`].
///
/// Nodes are addressed by [`NodeId`], which carries the slot's version in
/// addition to its index: when a node is dropped and its slot reused, ids
/// referring to the dropped node no longer resolve ([`NodeTree::get`] returns
/// `None`, and indexing panics) instead of aliasing the new occupant.
pub struct NodeTree {
    map: SlotMap<NodeKey, Node>,
    /// Bumped whenever box geometry may have moved (layout ran or a scroll
    /// offset changed). Values derived from `final_layout()` (e.g. hoisted
    /// paint child offsets) are memoised against it. Starts at 1 so that a
    /// stamp of 0 is never current.
    geometry_generation: AtomicU64,
    /// Bumped at the start of each box-tree construction pass. Facts derived
    /// from a node's subtree are memoised against it while `constructing` is
    /// set, as the DOM and styles can't change during the pass.
    construction_generation: AtomicU64,
    constructing: AtomicBool,
    /// Scroll offset of the viewport (the root element scrolls the viewport,
    /// so its offset lives here rather than on the node).
    viewport_scroll: crate::Point<f64>,
}

impl NodeTree {
    pub(crate) fn new() -> Self {
        Self {
            map: SlotMap::with_key(),
            geometry_generation: AtomicU64::new(1),
            construction_generation: AtomicU64::new(0),
            constructing: AtomicBool::new(false),
            viewport_scroll: crate::Point::ZERO,
        }
    }

    pub fn viewport_scroll(&self) -> crate::Point<f64> {
        self.viewport_scroll
    }

    /// Memoised paint geometry may depend on the viewport scroll, so this
    /// bumps the geometry generation like any other scroll write.
    pub(crate) fn set_viewport_scroll(&mut self, scroll: crate::Point<f64>) {
        self.viewport_scroll = scroll;
        self.bump_geometry_generation();
    }

    /// See the `geometry_generation` field.
    pub fn geometry_generation(&self) -> u64 {
        self.geometry_generation.load(Ordering::Relaxed)
    }

    pub(crate) fn bump_geometry_generation(&self) {
        self.geometry_generation.fetch_add(1, Ordering::Relaxed);
    }

    /// The current construction generation, or `None` outside a construction
    /// pass. See the `construction_generation` field.
    pub fn construction_generation(&self) -> Option<u64> {
        self.constructing
            .load(Ordering::Relaxed)
            .then(|| self.construction_generation.load(Ordering::Relaxed))
    }

    pub(crate) fn begin_construction(&self) {
        self.construction_generation.fetch_add(1, Ordering::Relaxed);
        self.constructing.store(true, Ordering::Relaxed);
    }

    pub(crate) fn end_construction(&self) {
        self.constructing.store(false, Ordering::Relaxed);
    }

    /// The number of live nodes in the map.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Whether `id` resolves to a live node.
    pub fn contains_key(&self, id: NodeId) -> bool {
        self.map.contains_key(to_key(id))
    }

    /// Get a reference to the node with the given id, if it is still live.
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.map.get(to_key(id))
    }

    /// Get a mutable reference to the node with the given id, if it is still live.
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.map.get_mut(to_key(id))
    }

    /// Insert a node constructed with knowledge of its own id.
    pub(crate) fn insert_with_key(&mut self, f: impl FnOnce(NodeId) -> Node) -> NodeId {
        to_id(self.map.insert_with_key(|key| f(to_id(key))))
    }

    /// Remove the node with the given id, returning it if it was still live.
    pub(crate) fn remove(&mut self, id: NodeId) -> Option<Node> {
        self.map.remove(to_key(id))
    }

    /// Iterate over all live `(NodeId, &Node)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.map.iter().map(|(key, node)| (to_id(key), node))
    }

    /// Iterate over all live `(NodeId, &mut Node)` pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (NodeId, &mut Node)> {
        self.map.iter_mut().map(|(key, node)| (to_id(key), node))
    }
}

impl Index<NodeId> for NodeTree {
    type Output = Node;

    #[track_caller]
    #[inline]
    fn index(&self, id: NodeId) -> &Node {
        &self.map[to_key(id)]
    }
}

impl IndexMut<NodeId> for NodeTree {
    #[track_caller]
    #[inline]
    fn index_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.map[to_key(id)]
    }
}
