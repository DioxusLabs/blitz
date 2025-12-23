//! Text selection state and logic for non-input elements.
//!
//! This module handles text selection across inline roots in the document,
//! including support for anonymous blocks which have unstable IDs across
//! layout reconstruction.

/// Represents one endpoint (anchor or focus) of a text selection.
#[derive(Clone, Debug, Default)]
pub struct SelectionEndpoint {
    /// The inline root node containing this endpoint
    pub node: Option<usize>,
    /// Byte offset within the inline root's text
    pub offset: usize,
    /// Parent element ID (stable reference for anonymous blocks)
    pub parent: Option<usize>,
    /// Index among anonymous siblings (for anonymous blocks only)
    pub sibling_index: Option<usize>,
}

impl SelectionEndpoint {
    /// Create a new endpoint at the given node and offset
    fn new(node: usize, offset: usize) -> Self {
        Self {
            node: Some(node),
            offset,
            parent: None,
            sibling_index: None,
        }
    }

    /// Check if this endpoint has a valid node
    pub fn is_some(&self) -> bool {
        self.node.is_some()
    }

    /// Clear this endpoint
    pub fn clear(&mut self) {
        self.node = None;
        self.offset = 0;
        self.parent = None;
        self.sibling_index = None;
    }

    /// Set the anonymous block info for this endpoint
    pub fn set_anonymous_info(&mut self, parent: Option<usize>, sibling_index: Option<usize>) {
        self.parent = parent;
        self.sibling_index = sibling_index;
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

    /// Check if there is an active (non-empty) selection
    pub fn is_active(&self) -> bool {
        self.anchor.is_some()
            && self.focus.is_some()
            && (self.anchor.node != self.focus.node || self.anchor.offset != self.focus.offset)
    }

    /// Clear the selection
    pub fn clear(&mut self) {
        self.anchor.clear();
        self.focus.clear();
    }

    /// Update the focus endpoint
    pub fn set_focus(&mut self, node: usize, offset: usize) {
        self.focus.node = Some(node);
        self.focus.offset = offset;
    }
}
