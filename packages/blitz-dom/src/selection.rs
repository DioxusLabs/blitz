//! Text selection state and logic for non-input elements.
//!
//! This module handles text selection across inline roots in the document,
//! including support for anonymous blocks which have unstable IDs across
//! layout reconstruction.

/// Represents one endpoint (anchor or focus) of a text selection.
///
/// For regular nodes, `node_or_parent` contains the node ID directly.
/// For anonymous blocks, `node_or_parent` contains the parent ID and
/// `sibling_index` contains the index among anonymous siblings.
#[derive(Clone, Debug, Default)]
pub struct SelectionEndpoint {
    /// For regular nodes: the node ID directly.
    /// For anonymous blocks: the parent ID (requires lookup via sibling_index).
    pub(crate) node_or_parent: Option<usize>,
    /// Byte offset within the inline root's text
    pub offset: usize,
    /// For anonymous blocks only: index among anonymous siblings.
    /// When Some, node_or_parent is a parent ID requiring lookup.
    pub(crate) sibling_index: Option<usize>,
}

impl SelectionEndpoint {
    /// Create a new endpoint at the given node and offset (for non-anonymous nodes)
    fn new(node: usize, offset: usize) -> Self {
        Self {
            node_or_parent: Some(node),
            offset,
            sibling_index: None,
        }
    }

    /// Check if this endpoint is set
    pub fn is_some(&self) -> bool {
        self.node_or_parent.is_some()
    }

    /// Clear this endpoint
    pub fn clear(&mut self) {
        self.node_or_parent = None;
        self.offset = 0;
        self.sibling_index = None;
    }

    /// Resolve the node ID, using a lookup function for anonymous blocks.
    pub fn resolve_node_id(
        &self,
        lookup_fn: impl FnOnce(usize, usize) -> Option<usize>,
    ) -> Option<usize> {
        match (self.node_or_parent, self.sibling_index) {
            (Some(node), None) => Some(node),
            (Some(parent), Some(idx)) => lookup_fn(parent, idx),
            _ => None,
        }
    }

    /// Set as a direct node reference (for regular nodes)
    pub fn set_node(&mut self, node: usize, offset: usize) {
        self.node_or_parent = Some(node);
        self.offset = offset;
        self.sibling_index = None;
    }

    /// Set as an anonymous block reference
    pub fn set_anonymous(&mut self, parent: usize, sibling_index: usize, offset: usize) {
        self.node_or_parent = Some(parent);
        self.offset = offset;
        self.sibling_index = Some(sibling_index);
    }
}

/// Text selection state for non-input elements.
///
/// Tracks both the anchor (where selection started) and focus (where it currently ends).
/// For anonymous blocks, we store stable parent references since anonymous block IDs
/// can change during layout reconstruction.
#[derive(Clone, Debug, Default)]
pub struct TextSelection {
    /// The anchor point (where selection started via mousedown)
    pub anchor: SelectionEndpoint,
    /// The focus point (where selection currently ends, updated during drag)
    pub focus: SelectionEndpoint,
}

impl TextSelection {
    /// Create a selection spanning from anchor to focus
    pub fn new(
        anchor_node: usize,
        anchor_offset: usize,
        focus_node: usize,
        focus_offset: usize,
    ) -> Self {
        Self {
            anchor: SelectionEndpoint::new(anchor_node, anchor_offset),
            focus: SelectionEndpoint::new(focus_node, focus_offset),
        }
    }

    /// Check if there is an active (non-empty) selection.
    /// Note: For anonymous blocks this compares parent+sibling_index, not resolved node IDs.
    pub fn is_active(&self) -> bool {
        self.anchor.is_some()
            && self.focus.is_some()
            && (self.anchor.node_or_parent != self.focus.node_or_parent
                || self.anchor.sibling_index != self.focus.sibling_index
                || self.anchor.offset != self.focus.offset)
    }

    /// Clear the selection
    pub fn clear(&mut self) {
        self.anchor.clear();
        self.focus.clear();
    }

    /// Update the focus endpoint (for non-anonymous nodes)
    pub fn set_focus(&mut self, node: usize, offset: usize) {
        self.focus.set_node(node, offset);
    }
}
