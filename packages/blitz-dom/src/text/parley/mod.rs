//! The Parley text backend.

mod document;
mod editing;
mod inline;
pub(crate) mod stylo_to_parley;

use blitz_traits::node_id::NodeId;

/// The Parley crate, for the renderer's Parley painter.
pub use ::parley;
pub use document::{FontContext, TextContext};
pub use editing::TextEditor;
pub use inline::{MarkerLayout, TextLayout};

/// Parley Brush type for Blitz which contains the Blitz node id
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextBrush {
    /// The node id for the span
    pub id: NodeId,
}

impl TextBrush {
    pub(crate) fn from_id(id: NodeId) -> Self {
        Self { id }
    }
}
