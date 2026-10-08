//! The Parley text backend.

mod build;
mod editor;
mod fonts;
mod layout;
pub(crate) mod style;

#[cfg(feature = "parallel-construct")]
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use blitz_traits::node_id::NodeId;
use kurbo::{Rect, Size};
#[cfg(not(feature = "winkin"))]
use parley::{Affinity, BreakReason, Cluster, ClusterSide, Cursor, Run, Selection};
use parley::{ContentWidths, LayoutContext, PositionedLayoutItem};
#[cfg(feature = "parallel-construct")]
use thread_local::ThreadLocal;

use super::{InlineLayoutEngine, InlineText};
use crate::node::{InlineContent, InlineTextHit, Node};

/// The Parley crate, for the renderer's Parley painter.
pub use ::parley;
pub use ::parley::FontContext;
pub use editor::TextEditor;
pub use fonts::FaceDescriptors;
pub(crate) use fonts::face_descriptors;

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
    pub text: String,
    pub content_widths: Option<ContentWidths>,
    pub layout: parley::Layout<TextBrush>,
    /// Block-axis offset (in CSS px) of the line boxes from the top of the container's content
    /// box, as applied by `align-content`.
    pub block_offset: f32,
    /// The same content laid out by winkin, which measuring, breaking and
    /// painting read in place of `layout`.
    #[cfg(feature = "winkin")]
    pub winkin: crate::text_winkin::WinkinText,
}

impl TextLayout {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn content_widths(&mut self) -> ContentWidths {
        *self
            .content_widths
            .get_or_insert_with(|| self.layout.calculate_content_widths())
    }
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextLayout")
    }
}

impl InlineText for TextLayout {
    fn scale(&self) -> f32 {
        #[cfg(feature = "winkin")]
        {
            self.winkin.scale()
        }
        #[cfg(not(feature = "winkin"))]
        self.layout.scale()
    }

    fn block_offset(&self) -> f32 {
        self.block_offset
    }

    fn text(&self) -> &str {
        &self.text
    }

    fn text_len(&self) -> usize {
        #[cfg(feature = "winkin")]
        {
            self.winkin.layout().map_or(0, |layout| layout.text().len())
        }
        #[cfg(not(feature = "winkin"))]
        self.text.len()
    }

    fn selected_text(&self, start: usize, end: usize) -> Option<String> {
        #[cfg(feature = "winkin")]
        {
            let layout = self.winkin.layout()?;
            (start < end && end <= layout.text().len()).then(|| {
                layout
                    .selected_text(start..end, winkin::selection::CopyKind::Text)
                    .to_string()
            })
        }
        #[cfg(not(feature = "winkin"))]
        {
            self.text.get(start..end).map(str::to_owned)
        }
    }

    fn logical_content(&self) -> impl Iterator<Item = InlineContent<'_>> {
        #[cfg(feature = "winkin")]
        {
            self.winkin
                .layout()
                .into_iter()
                .flat_map(crate::text_winkin::logical_content)
        }
        #[cfg(not(feature = "winkin"))]
        {
            let text = self.text.as_str();
            let mut lines = self.layout.lines();
            let mut boxes = self.layout.inline_boxes().peekable();
            // The current line's runs, sorted from visual into logical order for bidi text
            let mut runs: Vec<Run<'_, TextBrush>> = Vec::new();
            let mut next_run = 0;
            let mut clusters = None;
            std::iter::from_fn(move || {
                loop {
                    if clusters.is_none() {
                        while next_run == runs.len() {
                            let Some(line) = lines.next() else {
                                // The boxes after the last line's text
                                let ibox = boxes.next()?;
                                return Some(InlineContent::Box(NodeId::from_u64(ibox.id)));
                            };
                            runs.clear();
                            runs.extend(line.runs());
                            runs.sort_by_key(|run| run.text_range().start);
                            next_run = 0;
                        }
                        clusters = Some(runs[next_run].clusters().peekable());
                        next_run += 1;
                    }
                    let run_clusters = clusters.as_mut()?;
                    let Some(cluster) = run_clusters.peek() else {
                        clusters = None;
                        continue;
                    };
                    // The boxes before the cluster come first
                    let range = cluster.text_range();
                    if let Some(ibox) = boxes.next_if(|ibox| ibox.index <= range.start) {
                        return Some(InlineContent::Box(NodeId::from_u64(ibox.id)));
                    }
                    let node_id = cluster.style().brush.id;
                    run_clusters.next();
                    return Some(InlineContent::Text {
                        node_id,
                        start: range.start,
                        text: text[range].into(),
                    });
                }
            })
        }
    }

    fn source_offset(&self, node_id: NodeId, offset: usize) -> Option<usize> {
        #[cfg(feature = "winkin")]
        {
            let layout = self.winkin.layout()?;
            let position = layout.position(
                winkin::NodeKey(node_id.as_u64()),
                offset,
                winkin::selection::Affinity::Downstream,
            )?;
            Some(position.offset)
        }
        #[cfg(not(feature = "winkin"))]
        {
            let _ = (node_id, offset);
            None
        }
    }

    fn maps_source(&self) -> bool {
        cfg!(feature = "winkin")
    }

    fn hit_test(
        &self,
        x: f32,
        y: f32,
        content_size: Size,
        scale: f32,
        exact: bool,
    ) -> Option<InlineTextHit> {
        #[cfg(feature = "winkin")]
        {
            crate::text_winkin::hit_test(
                &self.winkin,
                self.block_offset,
                x,
                y,
                content_size,
                scale,
                exact,
            )
        }
        #[cfg(not(feature = "winkin"))]
        {
            let _ = (content_size, scale);
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
    }

    fn selection_rects(
        &self,
        start: usize,
        end: usize,
        content_size: Size,
        scale: f32,
    ) -> Vec<Rect> {
        #[cfg(feature = "winkin")]
        {
            crate::text_winkin::selection_rects(&self.winkin, start, end, content_size, scale)
        }
        #[cfg(not(feature = "winkin"))]
        {
            let _ = (content_size, scale);
            let layout = &self.layout;
            let anchor = Cursor::from_byte_index(layout, start, Affinity::Downstream);
            let focus = Cursor::from_byte_index(layout, end, Affinity::Downstream);
            let selection = Selection::new(anchor, focus);
            let mut rects = Vec::new();
            selection.geometry_with(layout, |rect, _| {
                rects.push(Rect::new(rect.x0, rect.y0, rect.x1, rect.y1));
            });
            rects
        }
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

        // Under winkin, the box's own parts on each line are its rects.
        #[cfg(feature = "winkin")]
        let winkin_rects = self.winkin.layout().map(|winkin| {
            crate::text_winkin::fragment_rects(
                winkin,
                self.winkin.writing_mode(),
                root,
                node,
                origin_x,
                origin_y,
                scale,
            )
        });
        #[cfg(not(feature = "winkin"))]
        let winkin_rects: Option<std::iter::Empty<taffy::Rect<f32>>> = None;
        let lines = winkin_rects.is_none().then(|| layout.lines());

        // One rect per line box: the union of all of the target's fragments on that line
        let line_rects = lines.into_iter().flatten().filter_map(move |line| {
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
        });
        winkin_rects.into_iter().flatten().chain(line_rects)
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
    const SETS_WRITING_MODES: bool = cfg!(feature = "winkin");

    fn invalidate_content_widths(&mut self) {
        self.content_widths = None;
    }

    fn build_layouts(
        cx: &mut TextContext,
        nodes: &crate::NodeTree,
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
                layout.content_widths = None;
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
            layout.content_widths = None;
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

    fn compute_layout(
        state: &mut crate::layout::LayoutPassState<'_>,
        node_id: NodeId,
        layout: Box<Self>,
        frame: crate::layout::inline::Frame,
        block_ctx: &mut taffy::BlockContext<'_>,
    ) -> taffy::LayoutOutput {
        #[cfg(feature = "winkin")]
        return state.compute_inline_layout_winkin(node_id, layout, frame, block_ctx);
        #[cfg(not(feature = "winkin"))]
        return state.compute_inline_layout_parley(node_id, layout, frame, block_ctx);
    }
}
