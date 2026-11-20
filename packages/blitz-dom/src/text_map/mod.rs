//! White space collapsing for Parley's [`StyleRunBuilder`](parley::StyleRunBuilder) that records a
//! bidirectional map between source text offsets and layout text offsets, and finds the
//! hanging and removed white space at the end of each laid out line.
//!
//! Independent of the rest of Blitz: nodes are identified by a generic id, so this can be
//! lifted into its own crate.

mod collapse;
mod line_end;
mod offset_map;

pub use collapse::{TextChange, WhiteSpace, WhiteSpaceCollapser, is_segment_break};
pub use line_end::{LineEndWhitespace, line_end_whitespace};
pub use offset_map::{Affinity, TextOffsetMap};

/// The character Parley places in the text for each in-flow inline box.
pub const INLINE_BOX_CHAR: char = '\u{FFFC}';
