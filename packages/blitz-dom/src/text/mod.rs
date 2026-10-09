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
//! - [`FaceDescriptors`]: an `@font-face` rule's descriptors, which a backend registers a face by,
//!   in [`parlance`]'s font value types.
//!
//! Painting is the renderer's part: blitz-paint paints each backend's types in a module of its own,
//! which [`cfg_text_backend!`](crate::cfg_text_backend) selects.

use std::borrow::Cow;
use std::ops::Range;

use blitz_traits::net::Bytes;
use blitz_traits::node_id::NodeId;
use kurbo::{Rect, Size};
pub use parlance;
use parlance::{FontStyle, FontWeight};

use crate::node::{InlineContent, InlineTextHit, Node};

#[cfg(text_parley)]
pub mod parley;

#[cfg(text_parley)]
use self::parley as backend;

/// The fonts documents lay out text with. Clones share their fonts.
///
/// This is the text backend's own type, Parley's `FontContext` under Parley, and its methods
/// beyond [`TextFonts`] are the backend's: code written for every backend uses [`TextFonts`].
pub type FontContext = backend::FontContext;
/// An inline formatting context, as the text backend lays it out.
pub type TextLayout = backend::TextLayout;
/// The text of an `<input>` or `<textarea>`, as the text backend edits and lays it out.
pub type TextEditor = backend::TextEditor;
/// An outside list marker, as the text backend lays it out.
pub type MarkerLayout = backend::MarkerLayout;
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

/// Expands the arm for the text backend in use, and drops the others, so that a crate which
/// holds code for each backend compiles the one blitz-dom was built with.
///
/// Each arm is a backend's name and its tokens in braces. The arms of backends other than the one
/// in use are dropped, whether or not this build knows them, so callers can list every backend
/// they hold code for.
///
/// ```ignore
/// blitz_dom::cfg_text_backend! {
///     parley => { mod parley; use parley as backend; }
/// }
/// ```
#[macro_export]
macro_rules! cfg_text_backend {
    ($($arms:tt)*) => {
        $crate::__text_backend_arms! { $($arms)* }
    };
}

/// Expands the arm of [`cfg_text_backend!`] named for the text backend in use.
#[cfg(text_parley)]
#[doc(hidden)]
#[macro_export]
macro_rules! __text_backend_arms {
    () => {};
    (parley => { $($tokens:tt)* } $($rest:tt)*) => {
        $($tokens)*
        $crate::__text_backend_arms! { $($rest)* }
    };
    ($other:ident => { $($tokens:tt)* } $($rest:tt)*) => {
        $crate::__text_backend_arms! { $($rest)* }
    };
}

/// The fonts documents lay out text with.
pub trait TextFonts: Clone + Default {
    /// Fonts with only the fonts in `font_data`, decoded from WOFF or WOFF2 where it is, which
    /// every generic family resolves to, and no platform fonts: the setup for WASM, where browsers
    /// do not expose them.
    fn with_single_font(font_data: &[u8]) -> Self;

    /// Adds the fonts in `font_data`, decoded from WOFF or WOFF2 where it is, under the family
    /// names they declare. Fonts that need no decoding are kept without a copy.
    fn add_fonts(&mut self, font_data: impl Into<Bytes>);
}

/// What a document keeps for its text: its fonts, and the contexts the backend builds and
/// breaks text in.
pub(crate) trait DocumentText: Sized {
    /// The context for a document handed `fonts`, or the default fonts.
    fn new(fonts: Option<FontContext>) -> Self;

    /// The fonts the document was handed, as it hands them to its iframes.
    fn fonts(&self) -> FontContext;

    /// Adds a web font that has loaded, under its `@font-face` rule's descriptors.
    fn add_web_font(&mut self, bytes: Bytes, overrides: &crate::net::FontFaceOverrides);

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

    /// The text between byte offsets `start` and `end`, as it is copied, in the pieces of the
    /// laid-out text it is made of. Nothing where the range is not within the text.
    fn selected_text(&self, start: usize, end: usize) -> impl Iterator<Item = &str>;

    /// The laid-out text and inline boxes in logical order, as the lines place them, read as it
    /// is walked. Each stretch of text one node holds comes as one item, whose range indexes
    /// [`text`](Self::text), and each inline box comes once.
    fn logical_content(&self) -> impl Iterator<Item = InlineContent>;

    /// Where byte `offset` of text node `node_id`'s content falls in the laid-out text, where
    /// [`maps_source`](Self::maps_source).
    fn source_offset(&self, node_id: NodeId, offset: usize) -> Option<usize>;

    /// Whether the backend maps a text node's offsets into the laid-out text. Where it does not,
    /// callers follow white space collapsing and text transforms themselves.
    fn maps_source(&self) -> bool;

    /// The baseline of the first line, in CSS pixels below the top of the inline root's content
    /// box, or `None` where the layout has no lines.
    fn first_baseline(&self) -> Option<f32>;

    /// The text position at physical CSS-pixel coordinates relative to the inline root's content
    /// box. `exact` requires the point to be within a line and its text.
    fn hit_test(&self, x: f32, y: f32, exact: bool) -> Option<InlineTextHit>;

    /// Calls `f` with each rectangle that highlights the text between `start` and `end`, in
    /// device pixels relative to the inline root's content box moved down by
    /// [`block_offset`](Self::block_offset).
    fn for_each_selection_rect(&self, start: usize, end: usize, f: impl FnMut(Rect));

    /// The boxes `node`, a non-atomic inline element inside the inline root `root`, has on each
    /// line, first line first, in CSS pixels relative to `root`'s border box, computed as they are
    /// walked.
    fn fragment_rects(&self, root: &Node, node: &Node) -> impl Iterator<Item = taffy::Rect<f32>>;

    /// Prints the layout's lines and what they hold, for debugging.
    fn debug_print(&self);
}

/// What an element pushed as an inline span is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpanKind {
    /// A non-atomic inline box.
    Span,
    /// A `display: contents` element, which has no box but whose children inherit from it, or an
    /// annotation container that holds `<rt>`s of its own.
    Contents,
    /// A ruby container, `<ruby>`, where the backend sets ruby.
    Ruby,
    /// A ruby annotation, `<rt>` or an `<rtc>` holding no `<rt>`, where the backend sets ruby.
    /// `in_container` where it is an `<rt>` inside an `<rtc>`.
    Annotation { in_container: bool },
}

/// What a box pushed whole into the content of an inline formatting context is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InlineBoxKind {
    /// An atomic inline, which Taffy lays out and the lines place.
    Atomic,
    /// A float, which the lines flow around.
    Float,
    /// An absolutely positioned box, whose static position the lines find.
    Absolute,
}

/// The content of one inline formatting context as a text backend builds it, pushed in content
/// order by the walk of the DOM that Blitz runs for every backend.
///
/// Every span pushed is popped. The [`PushedText`](crate::layout::text_transform::PushedText)
/// methods are read only where the backend does not apply `text-transform` itself.
pub(crate) trait InlineBuilder: crate::layout::text_transform::PushedText {
    /// Whether the backend applies `text-transform` itself. Where it does, text is pushed as the
    /// DOM holds it; where it does not, Blitz transforms it first.
    const TRANSFORMS_TEXT: bool;

    /// Whether the backend sets ruby. Where it does, `<ruby>`, `<rt>` and `<rtc>` are pushed as
    /// ruby and the fallback `<rp>` is dropped; where it does not, they are spans.
    const SETS_RUBY: bool;

    /// Pushes the marker of a list item whose marker is inside it, before its content.
    fn push_marker(&mut self, marker: &crate::node::Marker);

    /// Opens a span for the element `node`, set in its computed style `style`.
    fn push_span(&mut self, node: &Node, style: &style::properties::ComputedValues, kind: SpanKind);

    /// Closes the span opened last, which was opened as `kind`.
    fn pop_span(&mut self, kind: SpanKind);

    /// Pushes the text of text node `node`.
    fn push_text(&mut self, node: &Node, text: &str);

    /// Pushes the element `node`, set in its computed style `style`, as a box of its own.
    fn push_inline_box(
        &mut self,
        node: &Node,
        style: &style::properties::ComputedValues,
        kind: InlineBoxKind,
    );

    /// Pushes the forced line break of the `<br>` element `node`, set in `style`.
    fn push_line_break(&mut self, node: &Node, style: &style::properties::ComputedValues);

    /// Pushes the line break opportunity of the `<wbr>` element `node`, and returns whether the
    /// backend took it. Where it did not, the element is pushed as a span.
    fn push_break_opportunity(&mut self, node: &Node) -> bool;
}

/// Building, measuring and placing an inline formatting context: the part of [`InlineText`]
/// layout runs.
pub(crate) trait InlineLayoutEngine: InlineText {
    /// Whether the backend sets lines in vertical writing modes itself, so that an inline
    /// formatting context runs in physical axes and meets Taffy's writing modes only at its edges.
    #[cfg_attr(not(feature = "writing-mode"), allow(dead_code))]
    const SETS_WRITING_MODES: bool;

    /// Builds the content of each inline formatting context in `layouts`, rooted at its node, from
    /// the DOM. This is the deferred, possibly parallel, part of box construction.
    fn build_layouts(
        cx: &mut TextContext,
        nodes: &crate::NodeTree,
        scale: f32,
        layouts: &mut [(NodeId, Box<Self>)],
    );

    /// Whether the content holds no text and no inline boxes.
    fn is_empty(&self) -> bool;

    /// Gives the inline boxes the sizes Taffy measured them at, in content order, and readies the
    /// lines of `style`, the inline root's computed style, to be broken. `text-indent` percentages
    /// count as zero until the lines are broken, and are then of the width they are broken in.
    fn prepare(&mut self, sizes: &[BoxMeasure], style: Option<&style::properties::ComputedValues>);

    /// Returns the content's min-content and max-content widths, in device pixels, with its
    /// floats: the widest of them is as wide as the content gets at min-content, and at
    /// max-content each paragraph is as wide as its text and the floats anchored in it.
    fn content_widths(&mut self) -> ContentWidths;

    /// Breaks the content into lines `width` device pixels wide, or as narrow as `exclusions`
    /// leaves them, placing each float into it as a line reaches it, and aligns them as `style`,
    /// the inline root's computed style, says.
    fn break_lines(
        &mut self,
        width: f32,
        style: Option<&style::properties::ComputedValues>,
        exclusions: &mut impl LineExclusions,
    );

    /// Returns how far the lines reach and where their baselines are, in device pixels from the
    /// content box's top.
    fn extent(&self) -> LinesExtent;

    /// Sets how far `align-content` moves the line boxes down the content box, in CSS pixels.
    fn set_block_offset(&mut self, offset: f32);

    /// Returns where the lines put each inline box, in the order they are on the lines.
    fn placements(&self) -> impl Iterator<Item = Placement>;

    /// Returns where the last line put its baseline when the lines were last broken, in device
    /// pixels from the content box's top, before `align-content` moved them.
    fn last_line_baseline(&self) -> LastBaseline;
}

/// Where a block container's last line box puts its baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LastBaseline {
    /// This far down.
    At(f32),
    /// It has no line box.
    None,
    /// Something in it the walk does not read, which Taffy answers for.
    Unknown,
}

/// An atomic inline or a float as Taffy measured it, which the lines are broken with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoxMeasure {
    /// The box's node.
    pub(crate) node: NodeId,
    /// Its border box, in CSS pixels.
    pub(crate) size: taffy::Size<f32>,
    /// Its margins, in CSS pixels.
    pub(crate) margin: taffy::Rect<f32>,
    /// Its baseline down from its border box's top, in CSS pixels, where it has one.
    pub(crate) baseline: Option<f32>,
    /// The side it floats to, where it is a float.
    pub(crate) float: Option<FloatSide>,
}

/// The min-content and max-content widths of an inline formatting context's content.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ContentWidths {
    pub(crate) min: f32,
    pub(crate) max: f32,
}

/// How far an inline formatting context's lines reach, and where their baselines are, in device
/// pixels from the content box's top left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LinesExtent {
    /// How far down the line boxes reach.
    pub(crate) height: f32,
    /// How far along the longest line reaches.
    pub(crate) width: f32,
    /// The first line's baseline, where there is a line.
    pub(crate) first_baseline: Option<f32>,
    /// The last line's baseline, where there is a line.
    pub(crate) last_baseline: Option<f32>,
}

/// Where the lines put an inline box, in device pixels from the content box's top left. Its style
/// says which of the positions apply.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    /// The box's node.
    pub(crate) node: NodeId,
    /// Where an atomic inline's margin box's left edge is, or where an inline-level absolutely
    /// positioned box would have been along its line.
    pub(crate) x: f32,
    /// Where an atomic inline's margin box's top edge is.
    pub(crate) top: f32,
    /// Where the line the box is on reaches from and to.
    pub(crate) line_top: f32,
    pub(crate) line_bottom: f32,
    /// Where a block-level absolutely positioned box would have started.
    pub(crate) block_start: f32,
}

/// The side of the block a float goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FloatSide {
    Left,
    Right,
}

/// A float the lines reached, to be placed: its margin box's size and the line's top, in device
/// pixels.
#[cfg_attr(not(feature = "floats"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FloatRequest {
    /// The key the content holds the float by.
    pub(crate) key: u64,
    pub(crate) side: FloatSide,
    pub(crate) inline_size: f32,
    pub(crate) block_size: f32,
    /// The top of the line that reached it, which it goes no higher than.
    pub(crate) block_start: f32,
}

/// Where a float was placed: its margin box, in device pixels from the content box's top left.
#[cfg_attr(not(feature = "floats"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlacedFloat {
    pub(crate) left: f32,
    pub(crate) right: f32,
    pub(crate) top: f32,
    pub(crate) bottom: f32,
}

/// The floats an inline formatting context's lines flow around, which also places the floats its
/// text reaches. Positions are in device pixels from the content box's top left.
#[cfg_attr(not(feature = "floats"), allow(dead_code))]
pub(crate) trait LineExclusions {
    /// Returns the left and right edges of the room a line has that reaches across the block from
    /// `start` to `end`.
    fn band(&self, start: f32, end: f32) -> (f32, f32);

    /// Returns the next position below `top` where the room changes, or `None` where it never
    /// does.
    fn below(&self, top: f32) -> Option<f32>;

    /// Places a float, which later bands account for, and returns where it went.
    fn place(&mut self, float: FloatRequest) -> PlacedFloat;
}

/// The size and scale of the laid-out text of an `<input>` or `<textarea>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorMetrics {
    /// How wide and tall the laid-out text is, in device pixels.
    pub size: Size,
    /// Device pixels per CSS pixel that the text is laid out at.
    pub scale: f32,
}

/// The text of an `<input>` or `<textarea>`, as it is read back.
pub trait EditableText {
    /// The text, with any text an input method is composing, which the selection indexes.
    fn raw_text(&self) -> &str;

    /// The text without any text an input method is composing, as a form submits it: borrowed
    /// where nothing is composing.
    fn text(&self) -> Cow<'_, str>;

    /// The selection's byte range in [`raw_text`](Self::raw_text).
    fn selection(&self) -> Range<usize>;

    /// The selected text, where the selection is not empty.
    fn selected_text(&self) -> Option<&str>;

    /// Whether the selection is a caret.
    fn is_selection_collapsed(&self) -> bool;

    /// The size and scale of the laid-out text, or `None` where it is not laid out.
    fn metrics(&self) -> Option<EditorMetrics>;

    /// The caret, in device pixels relative to the text's origin, where the editor shows one.
    fn caret_rect(&self) -> Option<Rect>;
}

/// Changing the text of an `<input>` or `<textarea>`: the part of [`EditableText`] the document
/// drives.
///
/// The setters record what changes, and the text is laid out again once, by the next
/// [`refresh`](Self::refresh) or [`edit`](Self::edit).
pub(crate) trait EditEngine: EditableText + Sized {
    /// An empty editor.
    fn new(is_multiline: bool) -> Self;

    /// Sets the text the editor starts with.
    fn set_initial_text(&mut self, text: &str);

    /// Sets the text, where it changed.
    fn set_text(&mut self, text: &str);

    /// Sets the text in `style`, `node`'s computed style, at `scale`, with lines unwrapped until a
    /// width is set.
    fn set_style(
        &mut self,
        node: NodeId,
        style: Option<&style::properties::ComputedValues>,
        scale: f32,
    );

    /// Sets the scale.
    fn set_scale(&mut self, scale: f32);

    /// Sets the width lines wrap at, in device pixels.
    fn set_width(&mut self, width: Option<f32>);

    /// Lays the text out again, where a setter changed it since it was last laid out.
    fn refresh(&mut self, cx: &mut TextContext);

    /// Applies `edit`, leaving the text laid out.
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
    /// Moves the caret by the first motion, then extends the selection by the second:
    /// `Select(LineStart, LineEnd)` selects the line.
    Select(Motion, Motion),
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
    #[inline]
    pub fn editor(&self) -> &TextEditor {
        self.editor
    }

    /// The text, with any text an input method is composing.
    #[inline]
    pub fn raw_text(&self) -> &str {
        self.editor.raw_text()
    }

    /// Applies `edit`.
    #[inline]
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

/// The descriptors of an `@font-face` rule that a text backend registers a face by, beyond its
/// family name, in the font value types the backends share.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceDescriptors {
    /// The `font-weight` descriptor, as its lowest and highest weight; a single weight is both.
    pub weight: Option<(FontWeight, FontWeight)>,
    /// The `font-style` descriptor, as its lowest and highest style; only an oblique's angle can
    /// differ between them.
    pub style: Option<(FontStyle, FontStyle)>,
}

impl FaceDescriptors {
    /// Reads the descriptors of an `@font-face` rule.
    pub(crate) fn from_rule(descriptors: &style::font_face::Descriptors) -> Self {
        use style::font_face::FontStyleRange;
        Self {
            weight: descriptors
                .font_weight
                .as_ref()
                .and_then(|range| range.compute())
                .map(|range| {
                    (
                        FontWeight::new(range.0.value()),
                        FontWeight::new(range.1.value()),
                    )
                }),
            style: descriptors.font_style.as_ref().map(|style| match style {
                FontStyleRange::Italic => (FontStyle::Italic, FontStyle::Italic),
                FontStyleRange::Oblique(min, max) => {
                    let (min, max) = (min.degrees(), max.degrees());
                    // Stylo parses `normal` as an oblique of no angle.
                    if min.is_none_or(|angle| angle == 0.0) && max.is_none_or(|angle| angle == 0.0)
                    {
                        (FontStyle::Normal, FontStyle::Normal)
                    } else {
                        (FontStyle::Oblique(min), FontStyle::Oblique(max.or(min)))
                    }
                }
            }),
        }
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
    use style::stylesheets::{CssRule, Origin, StylesheetInDocument as _};

    use parlance::{FontStyle, FontWeight};

    use super::{BACKEND, FaceDescriptors, TextBackend};
    use crate::{BaseDocument, DocumentConfig};

    /// The descriptors of an `@font-face` rule with `declarations`.
    fn face(declarations: &str) -> FaceDescriptors {
        let doc = BaseDocument::new(DocumentConfig::default());
        let css = format!("@font-face {{ font-family: face; {declarations} }}");
        let sheet = doc.make_stylesheet(css, Origin::Author);
        let guard = doc.guard.read();
        let rules = sheet.0.contents(&guard).rules(&guard);
        rules
            .iter()
            .find_map(|rule| match rule {
                CssRule::FontFace(face) => Some(FaceDescriptors::from_rule(
                    &face.read_with(&guard).descriptors,
                )),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn face_weight_is_a_value_or_a_range() {
        assert_eq!(face("").weight, None);
        let weight = |min, max| Some((FontWeight::new(min), FontWeight::new(max)));
        assert_eq!(face("font-weight: 700").weight, weight(700.0, 700.0));
        assert_eq!(face("font-weight: bold").weight, weight(700.0, 700.0));
        assert_eq!(face("font-weight: 300 600").weight, weight(300.0, 600.0));
    }

    #[test]
    fn face_style_is_normal_italic_or_oblique() {
        let style = |style| Some((style, style));
        assert_eq!(face("").style, None);
        assert_eq!(face("font-style: normal").style, style(FontStyle::Normal));
        assert_eq!(face("font-style: italic").style, style(FontStyle::Italic));
        assert_eq!(
            face("font-style: oblique").style,
            style(FontStyle::Oblique(Some(14.0)))
        );
        assert_eq!(
            face("font-style: oblique 10deg 20deg").style,
            Some((
                FontStyle::Oblique(Some(10.0)),
                FontStyle::Oblique(Some(20.0))
            ))
        );
    }

    /// The `parley` feature selects Parley.
    #[test]
    fn parley_is_the_backend() {
        assert_eq!(BACKEND, TextBackend::Parley);
    }
}
