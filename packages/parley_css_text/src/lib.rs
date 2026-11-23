//! White space collapsing for Parley's [`TreeBuilder`](parley::TreeBuilder) that records a
//! bidirectional map between source text offsets and layout text offsets, and finds the
//! hanging and removed white space at the end of each laid out line.
//!
//! Nodes are identified by a generic id, so this works with any document tree.

mod collapse;
mod line_end;
mod offset_map;
mod text_transform;

pub use collapse::{TextChange, WhiteSpace, WhiteSpaceCollapser, is_segment_break};
pub use line_end::{LineEndWhitespace, line_end_whitespace};
pub use offset_map::{Affinity, TextOffsetMap};
pub use text_transform::{
    CaseTransform, TextCase, TextTransform, TextTransformer, full_size_kana, full_width,
};

/// The character Parley places in the text for each in-flow inline box.
pub const INLINE_BOX_CHAR: char = '\u{FFFC}';
