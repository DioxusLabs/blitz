//! The Parley text backend.

mod build;
mod editor;
mod fonts;
mod layout;
pub(crate) mod style;

#[cfg(feature = "parallel-construct")]
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use std::ops::Range;

use blitz_traits::node_id::NodeId;
use kurbo::Rect;
use parley::{
    Affinity, BreakReason, Cluster, ClusterSide, Cursor, LayoutContext, PositionedLayoutItem, Run,
    Selection,
};
#[cfg(feature = "parallel-construct")]
use thread_local::ThreadLocal;

use super::{
    ContentWidths, FloatSide, InlineLayoutEngine, InlineText, LastBaseline, LineArea,
    LineExclusions, LineFlow, LinesExtent, LinesInputs, Placement,
};
use crate::node::{InlineContent, InlineTextHit, Node};

/// The Parley crate, for the renderer's Parley painter.
pub use ::parley;
pub use ::parley::FontContext;
pub use editor::TextEditor;

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

/// An outside list marker, laid out by Parley.
pub type MarkerLayout = parley::Layout<TextBrush>;

/// What a document keeps for its text under Parley.
pub struct TextContext {
    /// A Parley font context
    pub(crate) font_ctx: Arc<Mutex<FontContext>>,
    #[cfg(feature = "parallel-construct")]
    /// Thread-and-document-local copies to the font context
    pub(crate) thread_font_contexts: ThreadLocal<RefCell<Box<FontContext>>>,
    /// A Parley layout context
    pub(crate) layout_ctx: LayoutContext<TextBrush>,
}

// FIXME: static thread_local FontCtx isn't necessarily correct in multi-document context.
// Should use thread_local crate with ThreadLocal value store in the Document.
#[cfg(feature = "parallel-construct")]
thread_local! {
    static LAYOUT_CTX: RefCell<Option<Box<LayoutContext<TextBrush>>>> = const { RefCell::new(None) };
}

/// An inline formatting context laid out by Parley.
#[derive(Clone, Default)]
pub struct TextLayout {
    pub(crate) text: String,
    /// Parley's layout of the text and inline boxes, which the renderer paints.
    pub layout: parley::Layout<TextBrush>,
    /// Block-axis offset (in CSS px) of the line boxes from the top of the container's content
    /// box, as applied by `align-content`.
    pub(crate) block_offset: f32,
    /// The floats, as the lines place them: each one's node, side and margin box's size in device
    /// pixels.
    floats: Vec<(u64, FloatSide, f32, f32)>,
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextLayout")
    }
}

impl InlineText for TextLayout {
    #[inline]
    fn scale(&self) -> f32 {
        self.layout.scale()
    }

    #[inline]
    fn block_offset(&self) -> f32 {
        self.block_offset
    }

    #[inline]
    fn text(&self) -> &str {
        &self.text
    }

    #[inline]
    fn text_len(&self) -> usize {
        self.text.len()
    }

    #[inline]
    fn selected_text(&self, start: usize, end: usize) -> impl Iterator<Item = &str> {
        self.text.get(start..end).into_iter()
    }

    fn logical_content(&self) -> impl Iterator<Item = InlineContent> {
        let mut lines = self.layout.lines();
        let mut boxes = self.layout.inline_boxes().peekable();
        // The current line's runs, sorted from visual into logical order for bidi text
        let mut runs: Vec<Run<'_, TextBrush>> = Vec::new();
        let mut next_run = 0;
        let mut clusters = None;
        // The node of the last cluster's style, which the clusters after it mostly share
        let mut style = None;
        // A cluster read but not yet taken in, which waits for the items before it
        let mut held: Option<(NodeId, Range<usize>)> = None;
        // The text taken in since the last item, all of one node
        let mut pending: Option<(NodeId, Range<usize>)> = None;
        let text = |(node_id, range)| InlineContent::Text { node_id, range };
        std::iter::from_fn(move || {
            loop {
                let (node_id, range) = match held.take() {
                    Some(cluster) => cluster,
                    None => {
                        let Some(cluster) = clusters.as_mut().and_then(Iterator::next) else {
                            while next_run >= runs.len() {
                                let Some(line) = lines.next() else {
                                    // The text read last, then the boxes after the last line
                                    if let Some(read) = pending.take() {
                                        return Some(text(read));
                                    }
                                    let ibox = boxes.next()?;
                                    return Some(InlineContent::Box(NodeId::from_u64(ibox.id)));
                                };
                                runs.clear();
                                runs.extend(line.runs());
                                runs.sort_by_key(|run| run.text_range().start);
                                next_run = 0;
                            }
                            clusters = runs.get(next_run).map(Run::clusters);
                            next_run += 1;
                            continue;
                        };
                        let index = cluster.style_index();
                        let node_id = match style {
                            Some((styled, node_id)) if styled == index => node_id,
                            _ => {
                                let node_id = cluster.style().brush.id;
                                style = Some((index, node_id));
                                node_id
                            }
                        };
                        (node_id, cluster.text_range())
                    }
                };
                // The boxes before the cluster come first, after the text read before them
                if boxes.peek().is_some_and(|ibox| ibox.index <= range.start) {
                    held = Some((node_id, range));
                    if let Some(read) = pending.take() {
                        return Some(text(read));
                    }
                    if let Some(ibox) = boxes.next() {
                        return Some(InlineContent::Box(NodeId::from_u64(ibox.id)));
                    }
                    continue;
                }
                match &mut pending {
                    Some((read_id, read)) if *read_id == node_id && read.end == range.start => {
                        read.end = range.end;
                    }
                    _ => {
                        if let Some(read) = pending.replace((node_id, range)) {
                            return Some(text(read));
                        }
                    }
                }
            }
        })
    }

    #[inline]
    fn source_offset(&self, node_id: NodeId, offset: usize) -> Option<usize> {
        let _ = (node_id, offset);
        None
    }

    #[inline]
    fn maps_source(&self) -> bool {
        false
    }

    fn first_baseline(&self) -> Option<f32> {
        let line = self.layout.lines().next()?;
        Some(line.metrics().baseline / self.layout.scale() + self.block_offset)
    }

    fn hit_test(&self, x: f32, y: f32, exact: bool) -> Option<InlineTextHit> {
        let layout = &self.layout;
        let point = (x * layout.scale(), (y - self.block_offset) * layout.scale());
        let (cluster, side) = if exact {
            Cluster::from_point_exact(layout, point.0, point.1)?
        } else {
            Cluster::from_point(layout, point.0, point.1)?
        };
        let leading = side == ClusterSide::Left;
        let byte_offset = if cluster.is_rtl() {
            if leading {
                cluster.text_range().end
            } else {
                cluster.text_range().start
            }
        } else if leading || cluster.is_line_break() == Some(BreakReason::Explicit) {
            cluster.text_range().start
        } else {
            cluster.text_range().end
        };
        Some(InlineTextHit {
            node_id: cluster.style().brush.id,
            byte_offset,
        })
    }

    fn for_each_selection_rect(&self, start: usize, end: usize, mut f: impl FnMut(Rect)) {
        let layout = &self.layout;
        let anchor = Cursor::from_byte_index(layout, start, Affinity::Downstream);
        let focus = Cursor::from_byte_index(layout, end, Affinity::Downstream);
        let selection = Selection::new(anchor, focus);
        selection.geometry_with(layout, |rect, _| {
            f(Rect::new(rect.x0, rect.y0, rect.x1, rect.y1));
        });
    }

    fn fragment_rects(&self, root: &Node, node: &Node) -> impl Iterator<Item = taffy::Rect<f32>> {
        let layout = &self.layout;
        let scale = layout.scale();

        // Walk up the DOM parent chain from `id` to check whether it is (or is
        // inside) the target node, stopping at the inline root.
        let root_id = root.id;
        let is_in_target = move |mut id: NodeId| -> bool {
            loop {
                if id == node.id {
                    return true;
                }
                if id == root_id {
                    return false;
                }
                match node.with(id).parent {
                    Some(parent) => id = parent,
                    None => return false,
                }
            }
        };

        let root_layout = root.unrounded_layout();
        let content_box_inset = root_layout.padding + root_layout.border;
        let origin_x = content_box_inset.left;
        let origin_y = content_box_inset.top + self.block_offset;

        fn union(acc: &mut Option<taffy::Rect<f32>>, left: f32, top: f32, right: f32, bottom: f32) {
            *acc = Some(match *acc {
                Some(rect) => taffy::Rect {
                    left: rect.left.min(left),
                    top: rect.top.min(top),
                    right: rect.right.max(right),
                    bottom: rect.bottom.max(bottom),
                },
                None => taffy::Rect {
                    left,
                    top,
                    right,
                    bottom,
                },
            });
        }

        // One rect per line box: the union of all of the target's fragments on that line
        layout.lines().filter_map(move |line| {
            let line_metrics = line.metrics();
            let mut line_rect: Option<taffy::Rect<f32>> = None;

            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        if !is_in_target(glyph_run.style().brush.id) {
                            continue;
                        }
                        let x0 = glyph_run.offset();
                        let x1 = x0 + glyph_run.advance();
                        // Use the line box's block extent rather than the
                        // run's font ascent/descent: fonts with small
                        // typographic metrics would otherwise produce rects
                        // that clip the rendered glyphs. This matches the
                        // geometry used for text selection highlights.
                        let y0 = line_metrics.block_min_coord;
                        let y1 = line_metrics.block_max_coord;
                        union(&mut line_rect, x0, y0, x1, y1);
                    }
                    PositionedLayoutItem::InlineBox(inline_box) => {
                        if !is_in_target(NodeId::from_u64(inline_box.id)) {
                            continue;
                        }
                        let x0 = inline_box.x;
                        let y0 = inline_box.y;
                        union(
                            &mut line_rect,
                            x0,
                            y0,
                            x0 + inline_box.width,
                            y0 + inline_box.height,
                        );
                    }
                }
            }

            line_rect.map(|rect| taffy::Rect {
                left: origin_x + rect.left / scale,
                top: origin_y + rect.top / scale,
                right: origin_x + rect.right / scale,
                bottom: origin_y + rect.bottom / scale,
            })
        })
    }

    fn debug_print(&self) {
        println!("Size: {}x{}", self.layout.width(), self.layout.height());
        println!("Text content: {:?}", self.text);
        println!("Inline Boxes:");
        for ibox in self.layout.inline_boxes() {
            print!("(id: {}) ", ibox.id);
        }
        println!();
        println!("Lines:");
        for (i, line) in self.layout.lines().enumerate() {
            let metrics = line.metrics();
            let x = metrics.inline_min_coord;
            let y = metrics.block_min_coord;
            let w = metrics.inline_max_coord - metrics.inline_min_coord;
            let h = metrics.block_max_coord - metrics.block_min_coord;
            println!("Line {i}: x:{x} y:{y} width:{w} height:{h}");
            for item in line.items() {
                print!("  ");
                match item {
                    PositionedLayoutItem::GlyphRun(run) => {
                        print!(
                            "RUN (x: {}, w: {}) ",
                            run.offset().round(),
                            run.run().advance()
                        )
                    }
                    PositionedLayoutItem::InlineBox(ibox) => print!(
                        "BOX {:?} (id: {} x: {} y: {} w: {}, h: {})",
                        ibox.kind,
                        ibox.id,
                        ibox.x.round(),
                        ibox.y.round(),
                        ibox.width.round(),
                        ibox.height.round()
                    ),
                }
                println!();
            }
        }
    }
}

impl InlineLayoutEngine for TextLayout {
    const SETS_WRITING_MODES: bool = false;
    const READS_ROOM_ABOVE: bool = false;

    fn build_layouts(
        cx: &mut TextContext,
        nodes: &crate::NodeTree,
        _cascade: crate::text::DocumentCascade<'_>,
        scale: f32,
        layouts: &mut [(NodeId, Box<Self>)],
    ) {
        #[cfg(feature = "parallel-construct")]
        {
            use rayon::prelude::*;
            let cx = &*cx;
            layouts.par_iter_mut().for_each(|(node_id, layout)| {
                let mut layout_ctx = LAYOUT_CTX
                    .take()
                    .unwrap_or_else(|| Box::new(LayoutContext::new()));
                let mut font_ctx = cx
                    .thread_font_contexts
                    .get_or(|| RefCell::new(Box::new(cx.font_ctx.lock().unwrap().clone())))
                    .borrow_mut();
                build::build_inline_layout_into(
                    nodes,
                    &mut layout_ctx,
                    &mut font_ctx,
                    layout,
                    scale,
                    *node_id,
                );
                LAYOUT_CTX.set(Some(layout_ctx));
            });
        }
        #[cfg(not(feature = "parallel-construct"))]
        for (node_id, layout) in layouts {
            build::build_inline_layout_into(
                nodes,
                &mut cx.layout_ctx,
                &mut cx.font_ctx.lock().unwrap(),
                layout,
                scale,
                *node_id,
            );
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.holds_nothing()
    }

    fn prepare(&mut self, doc: &mut crate::BaseDocument, root: NodeId, lines: LinesInputs<'_>) {
        let style = doc.nodes[root].primary_styles();
        self.prepare_lines(lines.sizes, style.as_deref().map(|style| &**style));
    }

    #[inline]
    fn content_widths(&mut self) -> ContentWidths {
        self.widths()
    }

    fn break_lines(
        &mut self,
        _cx: &mut TextContext,
        area: LineArea,
        style: Option<&::style::properties::ComputedValues>,
        exclusions: &mut impl LineExclusions,
    ) {
        self.break_into_lines(area.width, style, exclusions);
    }

    #[inline]
    fn extent(&self, _end_padding: f32) -> LinesExtent {
        self.lines_extent()
    }

    #[inline]
    fn set_frame(&mut self, block_offset: f32, _content: taffy::Size<f32>) {
        self.block_offset = block_offset;
    }

    fn placements(&self) -> impl Iterator<Item = Placement> {
        self.box_placements()
    }

    fn last_line_baseline(&self) -> LastBaseline {
        match self.lines_extent().last_baseline {
            Some(baseline) => LastBaseline::At(baseline),
            None => LastBaseline::None,
        }
    }

    #[inline]
    fn line_flow(&self) -> LineFlow {
        LineFlow::Horizontal
    }

    #[inline]
    fn place_on_page(&self, along: [f32; 2], across: [f32; 2]) -> (f32, f32) {
        (along[0], across[0])
    }

    #[inline]
    fn room_below(&self) -> f32 {
        0.0
    }

    #[inline]
    fn float_node(key: u64) -> Option<NodeId> {
        Some(NodeId::from_u64(key))
    }

    #[inline]
    fn inline_shift(
        _doc: &crate::BaseDocument,
        _node: NodeId,
        _containing: taffy::Size<f32>,
        _rtl: bool,
    ) -> taffy::Point<f32> {
        taffy::Point::ZERO
    }
}
