//! Types configure developer inspection and debug tools

use crate::node_id::NodeId;

/// Configuration for debug overlays and other debugging tools
#[derive(Debug, Default, Clone, Copy)]
pub struct DevtoolSettings {
    /// Outline elements with different border colors depending on
    /// inner display style of that element
    pub show_layout: bool,
    /// Render browser-style colored overlay showing the content-box,
    /// padding, border, and margin of the hovered element
    pub highlight_hover: bool,
    /// Render browser-style colored overlay showing the content-box,
    /// padding, border, and margin of a specific node (set by e.g. a
    /// remote devtools inspector)
    pub highlight_node: Option<NodeId>,
    /// Element picker mode: mouse events are intercepted (not delivered to
    /// the page) and reported to a remote devtools inspector so the user
    /// can pick an element by hovering/clicking it
    pub element_picker: bool,
}

impl DevtoolSettings {
    /// Toggle the [`show_layout`](Self::show_layout) setting
    pub fn toggle_show_layout(&mut self) {
        self.show_layout = !self.show_layout
    }

    /// Toggle the [`highlight_hover`](Self::highlight_hover) setting
    pub fn toggle_highlight_hover(&mut self) {
        self.highlight_hover = !self.highlight_hover
    }
}

/// Receives notifications of DOM mutations made to a document, for devtools
/// that mirror the document tree (e.g. a remote inspector).
///
/// Only mutations to nodes that are in the document are reported. Methods are
/// called while the document is mutably borrowed, so implementations should
/// only record the mutation (e.g. buffer it) and read the document later.
pub trait DevtoolsProvider: Send + Sync + 'static {
    /// `node_id` was inserted into the child list of `parent_id`
    fn node_inserted(&self, parent_id: NodeId, node_id: NodeId) {
        let _ = (parent_id, node_id);
    }
    /// `node_id` was removed from the child list of `parent_id` (it may since
    /// have been dropped)
    fn node_removed(&self, parent_id: NodeId, node_id: NodeId) {
        let _ = (parent_id, node_id);
    }
    /// The attribute with the given local name was set or removed on `node_id`
    fn attribute_changed(&self, node_id: NodeId, name: &str) {
        let _ = (node_id, name);
    }
    /// The text of the text node `node_id` changed
    fn character_data_changed(&self, node_id: NodeId) {
        let _ = node_id;
    }
}
