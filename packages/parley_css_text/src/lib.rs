#![warn(missing_docs)]

//! CSS text processing for Parley's [`StyleRunBuilder`](parley::StyleRunBuilder): `text-transform`,
//! white space collapsing, and a map between offsets in the source text nodes and offsets in
//! the laid out text.
//!
//! For each inline formatting context, push its text nodes in order:
//!
//! 1. [`TextTransformer::transform`] applies a [`CaseTransform`] to a text node's content and
//!    reports where the output's length differs from its source as [`TextChange`]s.
//! 2. [`WhiteSpaceCollapser::push_text`] collapses the transformed text's white space, pushes
//!    style runs to the `StyleRunBuilder` and records where each part of the source ended up.
//!    Inline boxes, `<br>`s and generated text (e.g. list markers) have their own `push_*`
//!    methods.
//! 3. [`WhiteSpaceCollapser::finish`] returns the collapsed text (to pass to
//!    [`StyleRunBuilder::build_into`](parley::StyleRunBuilder::build_into)) and the
//!    [`TextOffsetMap`], which maps source offsets to layout offsets and back.
//!
//! After line breaking, [`line_end_whitespace`] finds the white space at the end of each line
//! that hangs or that CSS removes.
//!
//! Text nodes are identified by a generic `Id`, so this works with any document tree. The
//! text transforms and white space handling are CSS values, which callers convert from their
//! own style system.

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
