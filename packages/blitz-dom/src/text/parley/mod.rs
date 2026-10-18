//! The Parley text backend.

mod build;
mod editor;
mod fonts;
mod layout;
pub(crate) mod style;

#[cfg(feature = "parallel-construct")]
use std::cell::RefCell;
use std::sync::{Arc, Mutex, OnceLock};

use std::ops::Range;

use blitz_traits::node_id::NodeId;
use kurbo::Rect;
use parley::{
    Affinity, BreakReason, Cluster, ClusterSide, Cursor, LayoutContext, PositionedLayoutItem, Run,
    Selection,
};
use smallvec::SmallVec;
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
    /// The out-of-flow positions for which some inline span with an inline box below it is a
    /// containing block (as `SPAN_*_CB` flags). The inline root claims those boxes in its place.
    /// Set by each `RunMode::PerformLayout` run of inline layout.
    pub(crate) span_cb_flags: stylo_taffy::StyleFlags,
    /// The floats, as the lines place them: each one's node, side and margin box's size in device
    /// pixels.
    floats: Vec<(u64, FloatSide, f32, f32)>,
    /// Each node's own fragments on the lines, indexed the first time a fragment is asked for.
    /// Taken whenever the layout is built or its lines are broken again.
    fragments: OnceLock<FragmentIndex>,
}

/// The fragments of each node that the layout's items are of: the glyph runs of the node's own
/// style span, or the node's inline box, and nothing of the nodes inside it. It is made from the
/// lines alone, so the DOM can change under it: which nodes are inside the one asked about is
/// read from the live DOM when it is asked.
#[derive(Clone, Default)]
struct FragmentIndex {
    /// Sorted by node, and then by line.
    fragments: Vec<Fragment>,
}

/// The union of a node's own items on one line.
#[derive(Clone, Copy)]
struct Fragment {
    node: u64,
    line: usize,
    /// In device pixels relative to the layout.
    rect: taffy::Rect<f32>,
    /// Whether the items are the node's inline box, whose contents are laid out apart.
    inline_box: bool,
}

impl FragmentIndex {
    /// `node`'s own fragments, first line first.
    fn of(&self, node: NodeId) -> &[Fragment] {
        let node = node.as_u64();
        let start = self
            .fragments
            .partition_point(|fragment| fragment.node < node);
        let len = self.fragments[start..].partition_point(|fragment| fragment.node == node);
        &self.fragments[start..start + len]
    }
}

/// Grows `acc` to take in `rect`.
fn union(acc: &mut taffy::Rect<f32>, rect: taffy::Rect<f32>) {
    acc.left = acc.left.min(rect.left);
    acc.top = acc.top.min(rect.top);
    acc.right = acc.right.max(rect.right);
    acc.bottom = acc.bottom.max(rect.bottom);
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextLayout")
    }
}

impl TextLayout {
    /// Each node's own items on the lines, in one walk of the lines.
    fn index_fragments(&self) -> FragmentIndex {
        let mut fragments: Vec<Fragment> = Vec::new();
        for (line_index, line) in self.layout.lines().enumerate() {
            let line_metrics = line.metrics();
            for item in line.items() {
                let (node, rect, inline_box) = match item {
                    // A run's brush is that of the style span it is set in: the element whose
                    // text it is, or the inline root for text directly inside it. A text node
                    // has no span, so it has no fragments of its own.
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        let x0 = glyph_run.offset();
                        // Use the line box's block extent rather than the
                        // run's font ascent/descent: fonts with small
                        // typographic metrics would otherwise produce rects
                        // that clip the rendered glyphs. This matches the
                        // geometry used for text selection highlights.
                        let rect = taffy::Rect {
                            left: x0,
                            top: line_metrics.block_min_coord,
                            right: x0 + glyph_run.advance(),
                            bottom: line_metrics.block_max_coord,
                        };
                        (glyph_run.style().brush.id.as_u64(), rect, false)
                    }
                    PositionedLayoutItem::InlineBox(inline_box) => {
                        let rect = taffy::Rect {
                            left: inline_box.x,
                            top: inline_box.y,
                            right: inline_box.x + inline_box.width,
                            bottom: inline_box.y + inline_box.height,
                        };
                        (inline_box.id, rect, true)
                    }
                };
                match fragments.last_mut() {
                    Some(last) if last.node == node && last.line == line_index => {
                        union(&mut last.rect, rect);
                    }
                    _ => fragments.push(Fragment {
                        node,
                        line: line_index,
                        rect,
                        inline_box,
                    }),
                }
            }
        }
        // A stable sort, so a node's items on a line are united in the order they are in.
        fragments.sort_by_key(|fragment| (fragment.node, fragment.line));
        fragments.dedup_by(|fragment, kept| {
            let same = fragment.node == kept.node && fragment.line == kept.line;
            if same {
                union(&mut kept.rect, fragment.rect);
                kept.inline_box |= fragment.inline_box;
            }
            same
        });
        FragmentIndex { fragments }
    }

    /// The boxes `node` has on each line, first line first, in device pixels relative to the
    /// layout: on each line, the union of the items of `node` and of every node inside it in
    /// the live DOM, up to the inline root `root`, and not inside a node that is laid out apart.
    fn folded_fragments<'a>(&'a self, root: &Node, node: &Node) -> FoldedFragments<'a> {
        let index = self.fragments.get_or_init(|| self.index_fragments());
        // Whether a node's subtree is laid out apart from these lines: an inline box, which
        // is laid out on its own, or a node with lines of its own.
        let laid_out_apart = |node: &Node, fragments: &[Fragment]| {
            node.id != root.id
                && (fragments.iter().any(|fragment| fragment.inline_box)
                    || node.flags.is_inline_root())
        };

        // The first node found with fragments is borrowed; a second one starts a list of all of
        // them, which is sorted by line and folded into one rect per line at the end.
        let mut first: &[Fragment] = &[];
        let mut merged: Option<SmallVec<[Fragment; 8]>> = None;
        let mut take = |fragments: &'a [Fragment]| {
            if fragments.is_empty() {
                return;
            }
            if let Some(merged) = &mut merged {
                merged.extend_from_slice(fragments);
            } else if first.is_empty() {
                first = fragments;
            } else {
                let mut list = SmallVec::with_capacity(first.len() + fragments.len());
                list.extend_from_slice(first);
                list.extend_from_slice(fragments);
                merged = Some(list);
            }
        };

        // The node and every node whose DOM parent chain reaches it before the inline root:
        // its children and pseudo-elements whose parent it is, and theirs, read from the DOM as
        // it is now. Iterative, as an inline formatting context can be deep.
        let fragments = index.of(node.id);
        take(fragments);
        let mut stack: SmallVec<[NodeId; 16]> = SmallVec::new();
        if !laid_out_apart(node, fragments) {
            stack.push(node.id);
        }
        while let Some(id) = stack.pop() {
            let parent = node.with(id);
            let children = parent
                .before()
                .into_iter()
                .chain(parent.children.iter().copied())
                .chain(parent.after());
            for child_id in children {
                let child = node.with(child_id);
                // Text is set in its parent's style span, so a text node has no items of its
                // own, nor any children.
                if child.is_text_node() || child.parent != Some(id) || child_id == root.id {
                    continue;
                }
                let fragments = index.of(child_id);
                take(fragments);
                if !laid_out_apart(child, fragments)
                    && (!child.children.is_empty()
                        || child.before().is_some()
                        || child.after().is_some())
                {
                    stack.push(child_id);
                }
            }
        }

        match merged {
            None => FoldedFragments {
                own: first.iter(),
                merged: SmallVec::new().into_iter(),
            },
            Some(mut merged) => {
                // A min/max union is exact, so the rects are the same in any order.
                merged.sort_unstable_by_key(|fragment| fragment.line);
                merged.dedup_by(|fragment, kept| {
                    let same_line = fragment.line == kept.line;
                    if same_line {
                        union(&mut kept.rect, fragment.rect);
                    }
                    same_line
                });
                FoldedFragments {
                    own: [].iter(),
                    merged: merged.into_iter(),
                }
            }
        }
    }
}

/// A node's boxes on the lines, as [`TextLayout::folded_fragments`] finds them: borrowed from
/// the index where only one node has items, or else united from those of several.
struct FoldedFragments<'a> {
    own: std::slice::Iter<'a, Fragment>,
    merged: smallvec::IntoIter<[Fragment; 8]>,
}

impl Iterator for FoldedFragments<'_> {
    type Item = taffy::Rect<f32>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.own
            .next()
            .copied()
            .or_else(|| self.merged.next())
            .map(|fragment| fragment.rect)
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
        let scale = self.layout.scale();
        let root_layout = root.unrounded_layout();
        let content_box_inset = root_layout.padding + root_layout.border;
        let origin_x = content_box_inset.left;
        let origin_y = content_box_inset.top + self.block_offset;

        self.folded_fragments(root, node)
            .map(move |rect| taffy::Rect {
                left: origin_x + rect.left / scale,
                top: origin_y + rect.top / scale,
                right: origin_x + rect.right / scale,
                bottom: origin_y + rect.bottom / scale,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attribute, BaseDocument, DocumentConfig, NodeData, local_name, qual_name};
    use blitz_traits::shell::{ColorScheme, Viewport};

    /// The fragments of `node` as found by walking every item of the layout and the DOM parent
    /// chain of each, which the index replaces.
    fn scanned_fragment_rects(
        text: &TextLayout,
        root: &Node,
        node: &Node,
    ) -> Vec<taffy::Rect<f32>> {
        let layout = &text.layout;
        let scale = layout.scale();
        let is_in_target = |mut id: NodeId| loop {
            if id == node.id {
                return true;
            }
            if id == root.id {
                return false;
            }
            match node.with(id).parent {
                Some(parent) => id = parent,
                None => return false,
            }
        };
        let root_layout = root.unrounded_layout();
        let content_box_inset = root_layout.padding + root_layout.border;
        let origin_x = content_box_inset.left;
        let origin_y = content_box_inset.top + text.block_offset;
        let mut rects = Vec::new();
        for line in layout.lines() {
            let metrics = line.metrics();
            let mut line_rect: Option<taffy::Rect<f32>> = None;
            for item in line.items() {
                let (id, rect) = match item {
                    PositionedLayoutItem::GlyphRun(run) => (
                        run.style().brush.id,
                        taffy::Rect {
                            left: run.offset(),
                            top: metrics.block_min_coord,
                            right: run.offset() + run.advance(),
                            bottom: metrics.block_max_coord,
                        },
                    ),
                    PositionedLayoutItem::InlineBox(ibox) => (
                        NodeId::from_u64(ibox.id),
                        taffy::Rect {
                            left: ibox.x,
                            top: ibox.y,
                            right: ibox.x + ibox.width,
                            bottom: ibox.y + ibox.height,
                        },
                    ),
                };
                if !is_in_target(id) {
                    continue;
                }
                line_rect = Some(match line_rect {
                    Some(acc) => taffy::Rect {
                        left: acc.left.min(rect.left),
                        top: acc.top.min(rect.top),
                        right: acc.right.max(rect.right),
                        bottom: acc.bottom.max(rect.bottom),
                    },
                    None => rect,
                });
            }
            rects.extend(line_rect.map(|rect| taffy::Rect {
                left: origin_x + rect.left / scale,
                top: origin_y + rect.top / scale,
                right: origin_x + rect.right / scale,
                bottom: origin_y + rect.bottom / scale,
            }));
        }
        rects
    }

    /// Line room that no float takes from: the whole of the width.
    struct NoFloats(f32);

    impl LineExclusions for NoFloats {
        fn band(&self, _start: f32, _end: f32) -> (f32, f32) {
            (0.0, self.0)
        }

        fn below(&self, _top: f32) -> Option<f32> {
            None
        }

        fn place(&mut self, _float: crate::text::FloatRequest) -> crate::text::PlacedFloat {
            unreachable!("the content holds no floats")
        }

        fn checkpoint(&self) -> usize {
            0
        }

        fn rewind(&mut self, _to: usize) {}
    }

    /// `<style>div > span::before { content: "> " }</style>` and `<div style="width:120px">aaaa
    /// <span>bbbb bbbb <a>cccc cccc cccc cccc cccc cccc cccc cccc</a> dddd <i></i>
    /// <span style="display:inline-block"></span> eeee<br>ffff <b>gggg<br>hhhh</b></span>
    /// iiii</div>`, built without the HTML parser, which lives in blitz-html.
    fn make_doc() -> (BaseDocument, NodeId, Vec<NodeId>) {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        let root_id = doc.root_node().id;
        let style = |value: &str| Attribute {
            name: qual_name!("style"),
            value: value.to_string(),
        };

        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html", html), vec![]);
        let head = mutator.create_element(qual_name!("head", html), vec![]);
        let stylesheet = mutator.create_element(qual_name!("style", html), vec![]);
        let css = mutator.create_text_node("div > span::before { content: \"> \" }");
        mutator.append_children(stylesheet, &[css]);
        mutator.append_children(head, &[stylesheet]);
        let body = mutator.create_element(qual_name!("body", html), vec![style("margin:0")]);
        let div = mutator.create_element(qual_name!("div", html), vec![style("width:120px")]);
        let span = mutator.create_element(qual_name!("span", html), vec![]);
        let link = mutator.create_element(qual_name!("a", html), vec![]);
        let empty = mutator.create_element(qual_name!("i", html), vec![]);
        let inline_block = mutator.create_element(
            qual_name!("span", html),
            vec![style("display:inline-block;width:30px;height:10px")],
        );
        let br = mutator.create_element(qual_name!("br", html), vec![]);
        let bold = mutator.create_element(qual_name!("b", html), vec![]);
        let bold_br = mutator.create_element(qual_name!("br", html), vec![]);

        let texts = [
            "aaaa ",
            "bbbb bbbb ",
            // Long enough to wrap in the 120px div with any platform's default font.
            "cccc cccc cccc cccc cccc cccc cccc cccc",
            " dddd ",
            " ",
            " eeee",
            "ffff ",
            "gggg",
            "hhhh",
            " iiii",
        ]
        .map(|text| mutator.create_text_node(text));
        mutator.append_children(link, &[texts[2]]);
        mutator.append_children(bold, &[texts[7], bold_br, texts[8]]);
        mutator.append_children(
            span,
            &[
                texts[1],
                link,
                texts[3],
                empty,
                texts[4],
                inline_block,
                texts[5],
                br,
                texts[6],
                bold,
            ],
        );
        mutator.append_children(div, &[texts[0], span, texts[9]]);
        mutator.append_children(body, &[div]);
        mutator.append_children(html, &[head, body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);

        doc.resolve(0.0);
        (doc, div, vec![span, link, empty, bold])
    }

    /// The inline layout of the inline root `div`.
    fn inline_layout(doc: &BaseDocument, div: NodeId) -> &TextLayout {
        doc.nodes[div]
            .element_data()
            .and_then(|element| element.inline_layout_data.as_deref())
            .expect("the div is an inline root")
    }

    /// Checks that the inline root `div` and every node inside it in the live DOM, text,
    /// pseudo-elements and atomic inlines included, have the fragments the scan finds, and
    /// returns how many nodes were checked.
    fn assert_indexed_rects_match_the_scan(doc: &BaseDocument, div: NodeId) -> usize {
        let root = &doc.nodes[div];
        let text = inline_layout(doc, div);
        let mut stack = vec![div];
        let mut checked = 0;
        while let Some(id) = stack.pop() {
            let node = &doc.nodes[id];
            stack.extend(node.before().into_iter().chain(node.after()));
            stack.extend(node.children.iter().copied());
            let indexed: Vec<_> = text.fragment_rects(root, node).collect();
            assert_eq!(
                indexed,
                scanned_fragment_rects(text, root, node),
                "node {id:?}"
            );
            checked += 1;
        }
        checked
    }

    #[test]
    fn indexed_fragment_rects_match_a_scan_of_the_layout() {
        let (doc, div, inlines) = make_doc();
        let root = &doc.nodes[div];
        let text = inline_layout(&doc, div);
        assert!(
            text.layout.lines().count() >= 4,
            "the content should wrap and break onto several lines"
        );

        assert!(assert_indexed_rects_match_the_scan(&doc, div) >= 20);

        let [span, link, empty, bold] = inlines[..] else {
            unreachable!()
        };
        assert!(
            doc.nodes[span].before().is_some(),
            "the span has a ::before"
        );
        let rects = |id: NodeId| doc.nodes[id].inline_fragment_boxes().unwrap().count();
        assert!(rects(span) >= 4, "the span is on several lines");
        assert!(rects(link) >= 2, "the link wraps");
        assert!(rects(bold) >= 2, "the <br> in the <b> breaks it");
        assert_eq!(rects(empty), 0, "an empty inline has no items");

        // A query after the lines are broken again is answered from the new lines.
        let mut text = text.clone();
        assert!(text.fragments.get().is_some());
        let width = 60.0 * text.layout.scale();
        text.break_into_lines(width, None, &mut NoFloats(width));
        assert!(text.fragments.get().is_none());
        let node = &doc.nodes[span];
        let indexed: Vec<_> = text.fragment_rects(root, node).collect();
        assert_eq!(indexed, scanned_fragment_rects(&text, root, node));
    }

    /// The index holds only what the lines say; which nodes are inside which is read from the
    /// DOM as it is when asked, as the scan read it, so moving nodes about without laying the
    /// document out again changes the answers the same way.
    #[test]
    fn indexed_fragment_rects_follow_the_dom_without_a_relayout() {
        let (mut doc, div, inlines) = make_doc();
        let [span, link, empty, bold] = inlines[..] else {
            unreachable!()
        };
        assert_indexed_rects_match_the_scan(&doc, div);
        assert!(inline_layout(&doc, div).fragments.get().is_some());
        let rects = |doc: &BaseDocument, id: NodeId| {
            let root = &doc.nodes[div];
            inline_layout(doc, div)
                .fragment_rects(root, &doc.nodes[id])
                .collect::<Vec<_>>()
        };
        let span_before = rects(&doc, span);
        let bold_before = rects(&doc, bold);

        // Out of the span, to the end of the div; the <b> into the empty <i>; and the
        // inline-block out of the document.
        let inline_block = doc.nodes[span]
            .children
            .iter()
            .copied()
            .find(|&id| {
                doc.nodes[id]
                    .data
                    .is_element_with_tag_name(&local_name!("span"))
            })
            .unwrap();
        let mut mutator = doc.mutate();
        mutator.append_children(div, &[link]);
        mutator.append_children(empty, &[bold]);
        mutator.remove_node(inline_block);
        drop(mutator);

        // Nothing has been laid out again: the lines, and the index of them, are the old ones.
        let text = inline_layout(&doc, div);
        assert!(text.fragments.get().is_some());
        assert_eq!(doc.nodes[link].parent, Some(div));
        assert_ne!(rects(&doc, span), span_before, "the span lost the link");
        assert_eq!(rects(&doc, empty), bold_before, "the <i> holds the <b>");
        assert!(assert_indexed_rects_match_the_scan(&doc, div) >= 15);
        // The removed box is found by neither, the scan's walk up from it ending outside the
        // inline root.
        let root = &doc.nodes[div];
        let removed = &doc.nodes[inline_block];
        assert_eq!(
            text.fragment_rects(root, removed).collect::<Vec<_>>(),
            scanned_fragment_rects(text, root, removed),
        );
    }

    /// An anonymous block's inline content keeps the block it is in as its DOM parent, so the
    /// scan never reaches the anonymous block from its children's items, and the walk down
    /// from it follows only the children whose parent it is.
    #[test]
    fn indexed_fragment_rects_match_a_scan_in_an_anonymous_block() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        let root_id = doc.root_node().id;
        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html", html), vec![]);
        let body = mutator.create_element(qual_name!("body", html), vec![]);
        let div = mutator.create_element(qual_name!("div", html), vec![]);
        let span = mutator.create_element(qual_name!("span", html), vec![]);
        let block = mutator.create_element(qual_name!("p", html), vec![]);
        let texts = ["aaaa ", "bbbb", "cccc"].map(|text| mutator.create_text_node(text));
        mutator.append_children(span, &[texts[1]]);
        mutator.append_children(block, &[texts[2]]);
        mutator.append_children(div, &[texts[0], span, block]);
        mutator.append_children(body, &[div]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);
        doc.resolve(0.0);

        let anonymous = doc.nodes[span].inline_root_ancestor().unwrap().id;
        assert!(matches!(
            doc.nodes[anonymous].data,
            NodeData::AnonymousBlock(_)
        ));
        assert_eq!(doc.nodes[span].parent, Some(div));
        assert_eq!(assert_indexed_rects_match_the_scan(&doc, anonymous), 4);
    }
}
