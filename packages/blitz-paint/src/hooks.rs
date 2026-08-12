use anyrender::PaintScene;
use blitz_dom::NodeId;

/// Identifies the DOM node that owns a contiguous group of paint commands.
///
/// Node IDs are local to a document, so subdocuments are distinguished by
/// their document ID.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PaintNode {
    pub document_id: usize,
    pub node_id: NodeId,
}

impl PaintNode {
    /// Creates an identifier from a [`blitz_dom::BaseDocument::id`] and node ID.
    pub const fn new(document_id: usize, node_id: NodeId) -> Self {
        Self {
            document_id,
            node_id,
        }
    }
}

/// Hooks for selecting DOM paint subtrees and associating emitted commands
/// with their owning nodes.
///
/// A node may produce more than one scope because its commands need not be
/// contiguous in CSS paint order. Scopes are properly nested and every
/// `begin_node` call is followed by `end_node`. A scope can be empty when an
/// element has no visible paint of its own.
pub trait PaintHooks<S: PaintScene> {
    /// Returns whether paint owned by this node should be emitted.
    ///
    /// Returning `false` for a traversed element also skips its descendants.
    /// Callers selecting individual descendants must therefore retain their
    /// ancestor chain. Inline text owners are checked independently for each
    /// paint group. This method may be called more than once for a node and
    /// must return a consistent result during one paint operation.
    #[inline]
    fn should_paint(&self, _node: PaintNode) -> bool {
        true
    }

    /// Called immediately before a contiguous group of commands owned by a node.
    #[inline]
    fn begin_node(&mut self, _scene: &mut S, _node: PaintNode) {}

    /// Called immediately after a contiguous group of commands owned by a node.
    #[inline]
    fn end_node(&mut self, _scene: &mut S, _node: PaintNode) {}
}

/// A no-op hook used by [`crate::paint_scene`].
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopPaintHooks;

impl<S: PaintScene> PaintHooks<S> for NoopPaintHooks {}

#[inline]
pub(crate) fn with_node<S, H, F>(scene: &mut S, hooks: &mut H, node: PaintNode, paint: F)
where
    S: PaintScene,
    H: PaintHooks<S>,
    F: FnOnce(&mut S, &mut H),
{
    hooks.begin_node(scene, node);
    paint(scene, hooks);
    hooks.end_node(scene, node);
}
