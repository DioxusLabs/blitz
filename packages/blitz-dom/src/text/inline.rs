//! Inline text construction and layout, including list markers.

use blitz_traits::node_id::NodeId;
use kurbo::Rect;

use super::{TextContext, backend};
use crate::node::{InlineContent, InlineTextHit, Node};

/// An inline formatting context: the text backend's layout of its lines, and where Blitz put
/// them in the inline root's content box.
///
/// Its [`InlineText`] reads are relative to the inline root's content box, where the backend's
/// are relative to the top of its lines.
#[derive(Clone, Debug, Default)]
pub struct TextLayout {
    /// The lines, as the text backend lays them out.
    pub backend: backend::TextLayout,
    /// How far `align-content` moves the line boxes down the content box, in CSS pixels.
    pub(crate) block_offset: f32,
    /// The offset (in CSS px) between the content box's unrounded and rounded positions.
    pub content_box_subpixel_offset: taffy::Point<f32>,
    /// The out-of-flow positions for which some inline span with an inline box below it is a
    /// containing block (as `SPAN_*_CB` flags). The inline root claims those boxes in its place.
    /// Set by each `RunMode::PerformLayout` run of inline layout.
    pub(crate) span_cb_flags: stylo_taffy::StyleFlags,
}

impl TextLayout {
    /// How far `align-content` moves the line boxes down the content box, in CSS pixels.
    #[inline]
    pub fn block_offset(&self) -> f32 {
        self.block_offset
    }

    /// Builds the content of each inline formatting context in `layouts` from the DOM: see
    /// [`InlineLayoutEngine::build_layouts`].
    pub(crate) fn build_layouts(
        cx: &mut TextContext,
        nodes: &crate::NodeTree,
        scale: f32,
        layouts: &mut [(NodeId, Box<Self>)],
    ) {
        let mut backends: Vec<_> = layouts
            .iter_mut()
            .map(|(node_id, layout)| (*node_id, &mut layout.backend))
            .collect();
        backend::TextLayout::build_layouts(cx, nodes, scale, &mut backends);
    }
}

impl InlineText for TextLayout {
    #[inline]
    fn scale(&self) -> f32 {
        self.backend.scale()
    }

    #[inline]
    fn text(&self) -> &str {
        self.backend.text()
    }

    #[inline]
    fn text_len(&self) -> usize {
        self.backend.text_len()
    }

    fn selected_text(&self, start: usize, end: usize) -> impl Iterator<Item = &str> {
        self.backend.selected_text(start, end)
    }

    fn logical_content(&self) -> impl Iterator<Item = InlineContent> {
        self.backend.logical_content()
    }

    #[inline]
    fn source_offset(&self, node_id: NodeId, offset: usize) -> Option<usize> {
        self.backend.source_offset(node_id, offset)
    }

    fn first_baseline(&self) -> Option<f32> {
        Some(self.backend.first_baseline()? + self.block_offset)
    }

    fn hit_test(&self, x: f32, y: f32, exact: bool) -> Option<InlineTextHit> {
        self.backend.hit_test(x, y - self.block_offset, exact)
    }

    fn for_each_selection_rect(&self, start: usize, end: usize, f: impl FnMut(Rect)) {
        self.backend.for_each_selection_rect(start, end, f);
    }

    fn fragment_rects(&self, root: &Node, node: &Node) -> impl Iterator<Item = taffy::Rect<f32>> {
        let offset = self.block_offset;
        self.backend
            .fragment_rects(root, node)
            .map(move |rect| taffy::Rect {
                top: rect.top + offset,
                bottom: rect.bottom + offset,
                ..rect
            })
    }

    fn debug_print(&self) {
        self.backend.debug_print();
    }
}

/// An outside list marker, as the text backend lays it out.
pub type MarkerLayout = backend::MarkerLayout;

///
/// Positions in the layout's text are byte offsets into the backend's laid-out text, which
/// [`text_len`](Self::text_len) bounds; [`source_offset`](Self::source_offset) maps a text node's
/// own offsets into it where the backend can.
pub trait InlineText: Default + Clone + std::fmt::Debug {
    /// Device pixels per CSS pixel that the layout was built at.
    fn scale(&self) -> f32;

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

    /// Where byte `offset` of text node `node_id`'s content falls in the laid-out text. `None`
    /// where the layout holds none of that node's content.
    fn source_offset(&self, node_id: NodeId, offset: usize) -> Option<usize>;

    /// The baseline of the first line, in CSS pixels below the top of the inline root's content
    /// box (of the lines, for a backend's layout), or `None` where the layout has no lines.
    fn first_baseline(&self) -> Option<f32>;

    /// The text position at physical CSS-pixel coordinates relative to the inline root's content
    /// box (to the top of the lines, for a backend's layout). `exact` requires the point to be
    /// within a line and its text.
    fn hit_test(&self, x: f32, y: f32, exact: bool) -> Option<InlineTextHit>;

    /// Calls `f` with each rectangle that highlights the text between `start` and `end`, in
    /// device pixels relative to the top of the lines: the inline root's content box moved down
    /// by [`TextLayout::block_offset`].
    fn for_each_selection_rect(&self, start: usize, end: usize, f: impl FnMut(Rect));

    /// The boxes `node`, a non-atomic inline element inside the inline root `root`, has on each
    /// line, first line first, in CSS pixels relative to `root`'s border box (moved down to the
    /// top of the lines, for a backend's layout), computed as they are walked.
    fn fragment_rects(&self, root: &Node, node: &Node) -> impl Iterator<Item = taffy::Rect<f32>>;

    /// Prints the layout's lines and what they hold, for debugging.
    fn debug_print(&self);
}

/// What an element pushed as an inline span is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpanKind {
    /// A non-atomic inline box.
    Span,
    /// A `display: contents` element, which has no box but whose children inherit from it.
    Contents,
    /// A ruby container, `<ruby>`. A span to a backend that does not set ruby.
    Ruby,
    /// A ruby annotation, `<rt>` or an `<rtc>` holding no `<rt>`. `in_container` where it is an
    /// `<rt>` inside an `<rtc>`. A span to a backend that does not set ruby.
    Annotation { in_container: bool },
    /// An annotation container, `<rtc>`, that holds `<rt>`s of its own, each an annotation. A
    /// span to a backend that does not set ruby.
    AnnotationContainer,
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
/// Every span pushed is popped.
pub(crate) trait InlineBuilder {
    /// Pushes the marker of a list item whose marker is inside it, before its content.
    fn push_marker(&mut self, marker: &crate::node::Marker);

    /// Opens a span for the element `node`, set in its computed style `style`.
    fn push_span(&mut self, node: &Node, style: &style::properties::ComputedValues, kind: SpanKind);

    /// Closes the span opened last, which was opened as `kind`.
    fn pop_span(&mut self, kind: SpanKind);

    /// Pushes the text of text node `node` as the DOM holds it, set in its parent's computed
    /// style `style`. The backend applies `text-transform`.
    fn push_text(
        &mut self,
        node: &Node,
        text: &str,
        style: Option<&style::properties::ComputedValues>,
    );

    /// Pushes the element `node`, set in its computed style `style`, as a box of its own.
    fn push_inline_box(
        &mut self,
        node: &Node,
        style: &style::properties::ComputedValues,
        kind: InlineBoxKind,
    );

    /// Pushes the forced line break of the `<br>` element `node`, set in `style`.
    fn push_line_break(&mut self, node: &Node, style: &style::properties::ComputedValues);

    /// Pushes the line break opportunity of the `<wbr>` element `node`, which contributes
    /// nothing else: its pseudo-elements are not pushed.
    fn push_break_opportunity(&mut self, node: &Node);
}

/// Building, measuring and placing an inline formatting context: the part of [`InlineText`]
/// layout runs.
pub(crate) trait InlineLayoutEngine: InlineText {
    /// Builds the content of each inline formatting context in `layouts`, rooted at its node, from
    /// the DOM. This is the deferred, possibly parallel, part of box construction.
    fn build_layouts(
        cx: &mut TextContext,
        nodes: &crate::NodeTree,
        scale: f32,
        layouts: &mut [(NodeId, &mut Self)],
    );

    /// Whether the content holds no text and no inline boxes.
    fn is_empty(&self) -> bool;

    /// Gives the inline boxes the sizes Taffy measured them at, in content order, and readies the
    /// lines of `style`, the inline root's computed style, to be broken. `text-indent` percentages
    /// count as zero until the lines are broken, and are then of the width they are broken in.
    fn prepare(&mut self, sizes: &[BoxMeasure], style: Option<&style::properties::ComputedValues>);

    /// Returns the content's min-content and max-content widths, in CSS pixels, with its
    /// floats: the widest of them is as wide as the content gets at min-content, and at
    /// max-content each paragraph is as wide as its text and the floats anchored in it.
    fn content_widths(&mut self) -> ContentWidths;

    /// Breaks the content into lines `width` CSS pixels wide, or as narrow as `exclusions`
    /// leaves them, placing each float into it as a line reaches it, and aligns them as `style`,
    /// the inline root's computed style, says. `padding_border` is the CSS pixels of padding
    /// and border beside the lines, which scales how far content sized to fit exactly may round
    /// past `width` without wrapping.
    fn break_lines(
        &mut self,
        width: f32,
        padding_border: f32,
        style: Option<&style::properties::ComputedValues>,
        exclusions: &mut impl LineExclusions,
    );

    /// Returns how far the lines reach and where their baselines are, in CSS pixels from the
    /// content box's top.
    fn extent(&self) -> LinesExtent;

    /// Returns where the lines put each inline box, in the order they are on the lines.
    fn placements(&self) -> impl Iterator<Item = Placement>;
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
    /// The sides it clears, where it is a float.
    pub(crate) clear: FloatClear,
}

/// The min-content and max-content widths of an inline formatting context's content.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ContentWidths {
    /// The min-content width of the text and atomic inlines, without the floats.
    pub(crate) min: f32,
    /// The max-content width, with the floats.
    pub(crate) max: f32,
    /// The widest float's margin box.
    pub(crate) float_min: f32,
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

/// Where the lines put an inline box, in CSS pixels from the content box's top left. Its style
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

/// The sides whose earlier floats a float goes below: its `clear`.
#[cfg_attr(not(feature = "floats"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FloatClear {
    #[default]
    None,
    Left,
    Right,
    Both,
}

/// A float the lines reached, to be placed: its margin box's size and the line's top, in CSS
/// pixels.
#[cfg_attr(not(feature = "floats"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FloatRequest {
    /// The key the content holds the float by.
    pub(crate) key: u64,
    pub(crate) side: FloatSide,
    pub(crate) clear: FloatClear,
    pub(crate) inline_size: f32,
    pub(crate) block_size: f32,
    /// The top of the line that reached it, which it goes no higher than.
    pub(crate) block_start: f32,
}

/// Where a float was placed: its margin box, in CSS pixels from the content box's top left.
#[cfg_attr(not(feature = "floats"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlacedFloat {
    pub(crate) left: f32,
    pub(crate) right: f32,
    pub(crate) top: f32,
    pub(crate) bottom: f32,
}

/// The floats an inline formatting context's lines flow around, which also places the floats its
/// text reaches. Positions are in CSS pixels from the content box's top left.
#[cfg_attr(not(feature = "floats"), allow(dead_code))]
pub(crate) trait LineExclusions {
    /// Returns the room floats leave a line `height` tall whose top is at or below `top`, among the
    /// slots `search` allows.
    fn slot(&self, top: f32, height: f32, search: SlotSearch) -> LineSlot;

    /// Places a float, which later slots account for, and returns where it went.
    fn place(&mut self, float: FloatRequest) -> PlacedFloat;
}

/// Which slots [`LineExclusions::slot`] looks in.
#[cfg_attr(not(feature = "floats"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlotSearch {
    /// The first slot the line fits beside the floats in.
    Any,
    /// The first slot below the floats beside the slot whose [`LineSlot::segment`] this is.
    After(usize),
    /// Below all the floats.
    BelowFloats,
}

/// No floats: every line has the full width. What the lines are broken with where Blitz is built
/// without float layout.
#[cfg_attr(feature = "floats", allow(dead_code))]
pub(crate) struct NoLineExclusions;

impl LineExclusions for NoLineExclusions {
    fn slot(&self, top: f32, _height: f32, _search: SlotSearch) -> LineSlot {
        LineSlot {
            top,
            left: 0.0,
            width: f32::INFINITY,
            height: f32::INFINITY,
            segment: None,
        }
    }

    fn place(&mut self, float: FloatRequest) -> PlacedFloat {
        PlacedFloat {
            left: 0.0,
            right: float.inline_size,
            top: float.block_start,
            bottom: float.block_start + float.block_size,
        }
    }
}

/// `exclusions`, which measures in CSS pixels, asked by a backend that breaks lines in device
/// pixels: every question and answer is scaled on the way through.
pub(crate) struct DeviceExclusions<'a, E> {
    pub(crate) exclusions: &'a mut E,
    /// Device pixels a CSS pixel.
    pub(crate) scale: f32,
}

impl<E: LineExclusions> LineExclusions for DeviceExclusions<'_, E> {
    fn slot(&self, top: f32, height: f32, search: SlotSearch) -> LineSlot {
        let scale = self.scale;
        let slot = self.exclusions.slot(top / scale, height / scale, search);
        LineSlot {
            top: slot.top * scale,
            left: slot.left * scale,
            width: slot.width * scale,
            height: slot.height * scale,
            segment: slot.segment,
        }
    }

    fn place(&mut self, float: FloatRequest) -> PlacedFloat {
        let scale = self.scale;
        let placed = self.exclusions.place(FloatRequest {
            inline_size: float.inline_size / scale,
            block_size: float.block_size / scale,
            block_start: float.block_start / scale,
            ..float
        });
        PlacedFloat {
            left: placed.left * scale,
            right: placed.left * scale + float.inline_size,
            top: placed.top * scale,
            bottom: placed.top * scale + float.block_size,
        }
    }
}

/// The room a line has beside the floats, in CSS pixels from the content box's top left.
#[cfg_attr(not(feature = "floats"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LineSlot {
    pub(crate) top: f32,
    pub(crate) left: f32,
    /// As wide as the lines are, or wider, where no float is beside it.
    pub(crate) width: f32,
    pub(crate) height: f32,
    /// The stretch of floats the slot is beside, where any is.
    pub(crate) segment: Option<usize>,
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
