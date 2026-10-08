//! Text layout, text editing and fonts, behind one interface for every text backend.
//!
//! Blitz lays out text with one backend, chosen when it is built. Each backend is a module here
//! that implements the traits below for its own types, and the type aliases name the types of the
//! backend in use, so the rest of Blitz is written once, against the aliases and the traits.
//!
//! - [`FontContext`] ([`TextFonts`]): the fonts documents lay out text with.
//! - [`TextLayout`] ([`InlineText`]): an inline formatting context, built from the DOM, measured,
//!   broken into lines and placed, and read back for hit testing, selection and `innerText`.
//! - [`TextEditor`] ([`EditableText`]): the text of an `<input>` or `<textarea>`, and its editing.
//! - [`MarkerLayout`]: an outside list marker.
//! - [`FaceDescriptors`]: an `@font-face` rule's descriptors, as the backend registers a face.
//!
//! Painting is the renderer's part: blitz-paint paints each backend's types in a module of its own,
//! which [`cfg_text_backend!`](crate::cfg_text_backend) selects.

use std::ops::Range;

use blitz_traits::node_id::NodeId;
use kurbo::{Rect, Size};

use crate::node::{InlineContent, InlineTextHit, Node};

#[cfg(text_parley)]
pub mod parley;

#[cfg(text_parley)]
use self::parley as backend;

pub(crate) use backend::face_descriptors;

/// The fonts documents lay out text with. Clones share their fonts.
pub type FontContext = backend::FontContext;
/// An inline formatting context, as the text backend lays it out.
pub type TextLayout = backend::TextLayout;
/// The text of an `<input>` or `<textarea>`, as the text backend edits and lays it out.
pub type TextEditor = backend::TextEditor;
/// An outside list marker, as the text backend lays it out.
pub type MarkerLayout = backend::MarkerLayout;
/// An `@font-face` rule's descriptors, as the text backend registers a face by them.
pub type FaceDescriptors = backend::FaceDescriptors;
/// What a document keeps for its text: its fonts, and the backend's contexts.
pub(crate) type TextContext = backend::TextContext;

/// The text backends Blitz can be built with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextBackend {
    /// [Parley](https://github.com/linebender/parley), the `parley` feature.
    Parley,
}

/// The text backend this build of Blitz lays out text with.
#[cfg(text_parley)]
pub const BACKEND: TextBackend = TextBackend::Parley;

/// Expands the tokens for the text backend in use, and drops the others, so that a crate which
/// holds code for each backend compiles the one blitz-dom was built with.
///
/// ```ignore
/// blitz_dom::cfg_text_backend! {
///     parley => { mod parley; use parley as backend; }
/// }
/// ```
#[cfg(text_parley)]
#[macro_export]
macro_rules! cfg_text_backend {
    (parley => { $($parley:tt)* }) => { $($parley)* };
}

/// The fonts documents lay out text with.
pub trait TextFonts: Clone + Default {
    /// Fonts with only the fonts in `font_data`, decoded from WOFF or WOFF2 where it is, which
    /// every generic family resolves to, and no platform fonts: the setup for WASM, where browsers
    /// do not expose them.
    fn with_single_font(font_data: &[u8]) -> Self;

    /// Adds the fonts in `font_data`, decoded from WOFF or WOFF2 where it is, under the family
    /// names they declare.
    fn add_fonts(&mut self, font_data: &[u8]);
}

/// What a document keeps for its text: its fonts, and the contexts the backend builds and
/// breaks text in.
pub(crate) trait DocumentText: Sized {
    /// The context for a document handed `fonts`, or the default fonts.
    fn new(fonts: Option<FontContext>) -> Self;

    /// The fonts the document was handed, as it hands them to its iframes.
    fn fonts(&self) -> FontContext;

    /// Adds a web font that has loaded, under its `@font-face` rule's descriptors.
    fn add_web_font(
        &mut self,
        bytes: blitz_traits::net::Bytes,
        overrides: &crate::net::FontFaceOverrides,
    );

    /// What Stylo measures font-relative units (`ex`, `ch`, `cap`, `ic`) with.
    fn font_metrics_provider(&self) -> Box<dyn style::device::servo::FontMetricsProvider>;
}

/// An inline formatting context, laid out and read back.
///
/// Positions in the layout's text are byte offsets into the backend's laid-out text, which
/// [`text_len`](Self::text_len) bounds; [`source_offset`](Self::source_offset) maps a text node's
/// own offsets into it where the backend can.
pub trait InlineText: Default + Clone + std::fmt::Debug {
    /// Device pixels per CSS pixel that the layout was built at.
    fn scale(&self) -> f32;

    /// How far `align-content` moves the line boxes down the content box, in CSS pixels.
    fn block_offset(&self) -> f32;

    /// The laid-out text, which the layout's byte offsets index.
    fn text(&self) -> &str;

    /// The byte length of the laid-out text.
    fn text_len(&self) -> usize;

    /// The text between byte offsets `start` and `end`, as it is copied.
    fn selected_text(&self, start: usize, end: usize) -> Option<String>;

    /// The laid-out text and inline boxes in logical order, as the lines place them.
    fn logical_content(&self) -> impl Iterator<Item = InlineContent<'_>>;

    /// Where byte `offset` of text node `node_id`'s content falls in the laid-out text, where
    /// [`maps_source`](Self::maps_source).
    fn source_offset(&self, node_id: NodeId, offset: usize) -> Option<usize>;

    /// Whether the backend maps a text node's offsets into the laid-out text. Where it does not,
    /// callers follow white space collapsing and text transforms themselves.
    fn maps_source(&self) -> bool;

    /// The text position at physical CSS-pixel coordinates relative to the inline root's content
    /// box, which is `content_size` CSS pixels in size. `exact` requires the point to be within a
    /// line and its text.
    fn hit_test(
        &self,
        x: f32,
        y: f32,
        content_size: Size,
        scale: f32,
        exact: bool,
    ) -> Option<InlineTextHit>;

    /// The rectangles that highlight the text between `start` and `end`, in device pixels
    /// relative to the content box, which is `content_size` CSS pixels in size.
    fn selection_rects(
        &self,
        start: usize,
        end: usize,
        content_size: Size,
        scale: f32,
    ) -> Vec<Rect>;

    /// The boxes `node`, a non-atomic inline element inside the inline root `root`, has on each
    /// line, in CSS pixels relative to `root`'s border box.
    fn fragment_rects(&self, root: &Node, node: &Node) -> Vec<taffy::Rect<f32>>;

    /// Prints the layout's lines and what they hold, for debugging.
    fn debug_print(&self);
}

/// Building, measuring and placing an inline formatting context: the part of [`InlineText`]
/// layout runs.
pub(crate) trait InlineLayoutEngine: InlineText {
    /// Drops the cached intrinsic sizes, so that they are measured again.
    fn invalidate_content_widths(&mut self);

    /// Builds the content of each inline formatting context in `layouts`, rooted at its node, from
    /// the DOM. This is the deferred, possibly parallel, part of box construction.
    fn build_layouts(
        cx: &mut TextContext,
        nodes: &crate::NodeTree,
        scale: f32,
        layouts: &mut [(NodeId, Box<Self>)],
    );

    /// Measures the inline boxes, breaks the lines and places them and what they hold, in the room
    /// `frame` describes.
    fn compute_layout(
        state: &mut crate::layout::LayoutPassState<'_>,
        node_id: NodeId,
        layout: Box<Self>,
        frame: crate::layout::inline::Frame,
        block_ctx: &mut taffy::BlockContext<'_>,
    ) -> taffy::LayoutOutput;
}

/// The text of an `<input>` or `<textarea>`, as it is read back.
pub trait EditableText {
    /// The text, with any text an input method is composing, which the selection indexes.
    fn raw_text(&self) -> &str;

    /// The text without any text an input method is composing, as a form submits it.
    fn text(&self) -> String;

    /// The selection's byte range in [`raw_text`](Self::raw_text).
    fn selection(&self) -> Range<usize>;

    /// The selected text, where the selection is not empty.
    fn selected_text(&self) -> Option<&str>;

    /// Whether the selection is a caret.
    fn is_selection_collapsed(&self) -> bool;

    /// Device pixels per CSS pixel that the text is laid out at.
    fn scale(&self) -> f32;

    /// How wide and tall the laid-out text is, in device pixels, once it is laid out.
    fn size(&self) -> Option<Size>;

    /// The caret, in device pixels relative to the text's origin, where the editor shows one.
    fn caret_rect(&self) -> Option<Rect>;
}

/// Changing the text of an `<input>` or `<textarea>`: the part of [`EditableText`] the document
/// drives.
pub(crate) trait EditEngine: EditableText + Sized {
    /// An empty editor.
    fn new(is_multiline: bool) -> Self;

    /// Sets the text, without laying it out.
    fn set_initial_text(&mut self, text: &str);

    /// Sets the text, and lays it out again where it changed.
    fn set_text(&mut self, cx: &mut TextContext, text: &str);

    /// Sets the text in `style`, `node`'s computed style, at `scale`, and lays it out.
    fn set_style(
        &mut self,
        cx: &mut TextContext,
        node: NodeId,
        style: Option<&style::properties::ComputedValues>,
        scale: f32,
    );

    /// Sets the scale, and lays the text out again.
    fn set_scale(&mut self, cx: &mut TextContext, scale: f32);

    /// Sets the width lines wrap at, in device pixels, and lays the text out again.
    fn set_width(&mut self, cx: &mut TextContext, width: Option<f32>);

    /// Applies `edit`.
    fn edit(&mut self, cx: &mut TextContext, edit: Edit<'_>);
}

/// Laying out an outside list marker.
pub(crate) trait MarkerEngine: Sized {
    /// Lays out `marker` for `node`, set in its computed style `style`, in the bullet font where
    /// `bullet`.
    fn build(
        cx: &mut TextContext,
        node: NodeId,
        style: &style::properties::ComputedValues,
        marker: &crate::node::Marker,
        bullet: bool,
        scale: f32,
    ) -> Self;
}

/// A change to the text or the selection of an `<input>` or `<textarea>`.
///
/// Points are in device pixels relative to the text's origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit<'a> {
    /// Replaces the selection with the text.
    Insert(&'a str),
    /// Deletes the selection, or the character after the caret.
    Delete,
    /// Deletes the selection, or the character before the caret.
    Backdelete,
    /// Deletes the selection, or the word after the caret.
    DeleteWord,
    /// Deletes the selection, or the word before the caret.
    BackdeleteWord,
    /// Deletes the selection.
    DeleteSelection,
    /// Moves the caret, collapsing the selection.
    Move(Motion),
    /// Moves the selection's focus, keeping its anchor.
    Extend(Motion),
    /// Selects all the text.
    SelectAll,
    /// Collapses the selection to its focus.
    CollapseSelection,
    /// Selects the bytes from the first offset to the second.
    SelectByteRange(usize, usize),
    /// Sets the text an input method is composing, and the cursor within it.
    SetCompose(&'a str, Option<(usize, usize)>),
    /// Clears the text an input method is composing.
    ClearCompose,
    /// Moves the caret to the point.
    MoveToPoint(f32, f32),
    /// Extends the selection to the point, as a drag does.
    ExtendSelectionToPoint(f32, f32),
    /// Extends the selection to the point, as a shift-click does.
    ShiftClickExtension(f32, f32),
    /// Selects the word at the point.
    SelectWordAtPoint(f32, f32),
    /// Selects the hard line at the point.
    SelectHardLineAtPoint(f32, f32),
}

/// Where an [`Edit::Move`] or an [`Edit::Extend`] takes the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// One character to the left.
    Left,
    /// One character to the right.
    Right,
    /// One word to the left.
    WordLeft,
    /// One word to the right.
    WordRight,
    /// One line up.
    Up,
    /// One line down.
    Down,
    /// The start of the visual line.
    LineStart,
    /// The end of the visual line.
    LineEnd,
    /// The start of the line, as the text's line breaks end it.
    HardLineStart,
    /// The end of the line, as the text's line breaks end it.
    HardLineEnd,
    /// The start of the text.
    TextStart,
    /// The end of the text.
    TextEnd,
}

/// An `<input>` or `<textarea>`'s editor with the document's text context, handed to
/// [`BaseDocument::with_text_input`](crate::BaseDocument::with_text_input).
pub struct TextInputDriver<'a> {
    pub(crate) editor: &'a mut TextEditor,
    pub(crate) cx: &'a mut TextContext,
}

impl TextInputDriver<'_> {
    /// The editor, to read back.
    pub fn editor(&self) -> &TextEditor {
        self.editor
    }

    /// The text, with any text an input method is composing.
    pub fn raw_text(&self) -> &str {
        self.editor.raw_text()
    }

    /// Applies `edit`.
    pub fn edit(&mut self, edit: Edit<'_>) {
        self.editor.edit(self.cx, edit);
    }

    /// Selects all the text.
    pub fn select_all(&mut self) {
        self.edit(Edit::SelectAll);
    }

    /// Selects the bytes from `start` to `end`.
    pub fn select_byte_range(&mut self, start: usize, end: usize) {
        self.edit(Edit::SelectByteRange(start, end));
    }
}

/// Fonts with only the fonts in `font_data`, which every generic family resolves to, and no
/// platform fonts: the standard setup for WASM, where browsers do not expose them. WOFF and WOFF2
/// are decoded.
pub fn build_single_font_ctx(font_data: &[u8]) -> FontContext {
    FontContext::with_single_font(font_data)
}

#[cfg(test)]
mod tests {
    use super::{BACKEND, TextBackend};

    /// The `parley` feature selects Parley.
    #[test]
    fn parley_is_the_backend() {
        assert_eq!(BACKEND, TextBackend::Parley);
    }
}
