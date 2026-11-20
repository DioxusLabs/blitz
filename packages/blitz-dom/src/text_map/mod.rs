//! White space collapsing for Parley's [`TreeBuilder`](parley::TreeBuilder) that records a
//! bidirectional map between source text offsets and layout text offsets.
//!
//! Independent of the rest of Blitz: nodes are identified by a generic id, so this can be
//! lifted into its own crate.

mod collapse;
mod offset_map;

pub use collapse::{WhiteSpace, WhiteSpaceCollapser, is_segment_break};
pub use offset_map::{Affinity, TextOffsetMap};

/// The character Parley places in the text for each in-flow inline box.
pub const INLINE_BOX_CHAR: char = '\u{FFFC}';
