//! Types configure developer inspection and debug tools

use crate::node_id::NodeId;
use smol_str::SmolStr;

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

/// A DOM mutation reported to a [`DevtoolsProvider`]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationRecord {
    NodeInserted(NodeInserted),
    NodeRemoved(NodeRemoved),
    AttributeChanged(AttributeChanged),
    CharacterDataChanged(CharacterDataChanged),
}

/// `node_id` was inserted into the child list of `parent_id`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeInserted {
    pub parent_id: NodeId,
    pub node_id: NodeId,
    /// The nearest preceding sibling at the time of insertion that devtools
    /// show (skipping whitespace-only text nodes and anonymous blocks), if any
    pub previous_sibling_id: Option<NodeId>,
}

/// `node_id` was removed from the child list of `parent_id` (it may since
/// have been dropped)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeRemoved {
    pub parent_id: NodeId,
    pub node_id: NodeId,
}

/// The attribute with the local name `name` was set or removed on `node_id`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeChanged {
    pub node_id: NodeId,
    pub name: SmolStr,
}

/// The text of the text or comment node `node_id` changed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterDataChanged {
    pub node_id: NodeId,
}

/// Receives the DOM mutations made to a document, for devtools that mirror
/// the document tree (e.g. a remote inspector).
///
/// Only mutations to nodes that are in the document are reported. They are
/// buffered by the document's mutator and delivered in a batch when it is
/// flushed (or dropped), while the document is still mutably borrowed, so
/// implementations should only record the mutations and read the document
/// later.
pub trait DevtoolsProvider: Send + Sync + 'static {
    /// A batch of mutations, in the order they were made
    fn mutations(&self, records: &[MutationRecord]) {
        let _ = records;
    }

    /// The document was dropped (e.g. a sub-document replaced by navigating
    /// its host element). Called from the document's `Drop` impl.
    fn document_dropped(&self) {}
}
