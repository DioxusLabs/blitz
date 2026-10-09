//! `text-overflow`: which lines of an inline context are too wide for their
//! box, and the shaped marker (`…` or a string) to draw at the cut.
//!
//! Split in two so that scrolling stays cheap:
//! - **post-layout** ([`overflowing_lines`] and [`Marker::from_layout`], called
//!   from `inline.rs` right after the lines are final): which lines overflow
//!   and where their content ends, plus the marker shaped once from the block
//!   container's style (css-overflow §5.2). Stored on the [`TextLayout`] as
//!   `Option<Box<TextOverflowLayout>>`.
//! - **at paint time** ([`resolve`]): the cut position for the current
//!   horizontal scroll offset, and whether the marker is needed at all — a
//!   scrolled box whose content end is in view shows no marker (css-overflow
//!   §5, "ellipsis-scrolling").
//!
//! Only the inline-end edge is handled: the right edge of a left-to-right
//! paragraph, the left edge of a right-to-left one (taken from the layout's
//! base direction, which is also what decides the side a line overflows on).
//!
//! Free of DOM and CSS types on purpose, so it can move into Parley later.
//!
//! [`TextLayout`]: crate::node::TextLayout

use parley::{Brush, Cluster, FontData, InlineBoxKind, Layout, Line, PositionedLayoutItem, Run};

/// Slack for float rounding when comparing positions (layout units).
const EPSILON: f32 = 0.01;

/// How far content may extend past its line box before it counts as
/// overflowing (layout units).
pub const OVERFLOW_SLACK: f32 = 0.5;

/// One shaped run of the marker (Parley may split it across fonts when the
/// block's font lacks a glyph).
#[derive(Clone, Debug)]
pub struct MarkerRun {
    pub font: FontData,
    pub font_size: f32,
    pub normalized_coords: Vec<i16>,
    /// Synthetic oblique angle in degrees, if the font needs one.
    pub skew: Option<f32>,
    /// Glyph ids with their x offset within the marker, in layout units.
    pub glyphs: Vec<(u32, f32)>,
}

/// The marker, shaped once per inline context in the **block container's**
/// style (css-overflow §5.2: styled and baseline-aligned according to the
/// block), so a small block cuts a large span with a small marker.
#[derive(Clone, Debug)]
pub struct Marker<B: Brush> {
    pub runs: Vec<MarkerRun>,
    pub advance: f32,
    pub brush: B,
}

impl<B: Brush> Marker<B> {
    /// Flattens a shaped marker layout (one line) into runs.
    pub fn from_layout(layout: &Layout<B>, brush: B) -> Option<Self> {
        let line = layout.lines().next()?;
        let mut runs = Vec::new();
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                let glyphs = run.positioned_glyphs().map(|g| (g.id, g.x)).collect();
                runs.push(MarkerRun {
                    font: run.run().font().clone(),
                    font_size: run.run().font_size(),
                    normalized_coords: run
                        .run()
                        .normalized_coords()
                        .iter()
                        .map(|c| c.to_bits())
                        .collect(),
                    skew: run.run().synthesis().skew(),
                    glyphs,
                });
            }
        }
        (!runs.is_empty()).then(|| Self {
            runs,
            advance: line.metrics().advance,
            brush,
        })
    }
}

/// A line whose content is wider than the box.
#[derive(Clone, Debug)]
pub struct TruncatedLine {
    pub line_index: usize,
    /// Where the line's content ends on the inline-end side (hanging
    /// whitespace excluded), in layout units: the right edge of the content
    /// in a left-to-right paragraph, the left edge in a right-to-left one.
    pub end: f32,
    /// The line's baseline — the block's, not a run's (`vertical-align`).
    pub baseline: f32,
}

/// Every candidate line of an inline context, the box width they are
/// measured against (each line is further limited to its own line box, which
/// floats may have shortened), and the marker they share.
#[derive(Clone, Debug)]
pub struct TextOverflowLayout<B: Brush> {
    pub max_width: f32,
    /// Whether the paragraph is right-to-left: lines overflow, and are cut,
    /// on the left.
    pub is_rtl: bool,
    pub marker: Marker<B>,
    pub lines: Vec<TruncatedLine>,
}

/// Where paint cuts a line for the current scroll position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    /// The edge of the last kept atom (layout units, unscrolled). Content
    /// beyond it — to the right, or to the left when `is_rtl` — is not
    /// painted.
    pub cut_x: f32,
    /// Where the marker starts: at the cut, or its own advance before it when
    /// `is_rtl`.
    pub marker_x: f32,
    pub is_rtl: bool,
}

impl Cut {
    /// Whether content spanning `start..end` (left to right) is hidden by
    /// this cut.
    pub fn hides(&self, start: f32, end: f32) -> bool {
        if self.is_rtl {
            start < self.cut_x - EPSILON
        } else {
            end > self.cut_x + EPSILON
        }
    }

    /// The part of `start..end` that is not hidden, if any.
    pub fn clamp(&self, start: f32, end: f32) -> Option<(f32, f32)> {
        let (start, end) = if self.is_rtl {
            (start.max(self.cut_x), end)
        } else {
            (start, end.min(self.cut_x))
        };
        (start < end).then_some((start, end))
    }
}

impl<B: Brush> TextOverflowLayout<B> {
    pub fn line(&self, index: usize) -> Option<&TruncatedLine> {
        self.lines.iter().find(|l| l.line_index == index)
    }
}

/// The inline extent content may occupy on a line: the line box (shortened by
/// floats, if any), within `0..max_width`.
fn line_bounds<B: Brush>(line: &Line<'_, B>, max_width: f32) -> (f32, f32) {
    let metrics = line.metrics();
    (
        metrics.inline_min_coord.max(0.0),
        metrics.inline_max_coord.min(max_width),
    )
}

/// Post-layout: the lines whose content ends outside their line box (see
/// [`line_bounds`], layout units) on the inline-end side.
pub fn overflowing_lines<B: Brush>(layout: &Layout<B>, max_width: f32) -> Vec<TruncatedLine> {
    let is_rtl = layout.is_rtl();
    let mut lines = Vec::new();
    for (line_index, line) in layout.lines().enumerate() {
        let metrics = line.metrics();
        let (min, max) = line_bounds(&line, max_width);
        let left = metrics.inline_min_coord + metrics.offset;
        // Hanging whitespace is at the inline-end of the line.
        let (end, overflows) = if is_rtl {
            let end = left + metrics.hanging_advance;
            (end, end < min - OVERFLOW_SLACK)
        } else {
            let end = left + metrics.advance - metrics.hanging_advance;
            (end, end > max + OVERFLOW_SLACK)
        };
        if overflows && line.items().next().is_some() {
            lines.push(TruncatedLine {
                line_index,
                end,
                baseline: metrics.baseline,
            });
        }
    }
    lines
}

/// Calls `visit(start, end)` for the atoms of the line that overlap the view
/// `view_start..view_end`, and possibly some of their neighbours, in no
/// particular order. Atoms are grapheme clusters (a ligature counts as one)
/// and in-flow inline boxes.
///
/// Runs and inline boxes outside the view are stepped over by their extent.
/// Clusters are only walked in the runs that overlap the view, from whichever
/// end of the run is nearer to it: beyond the view itself, that is at most
/// the shorter of the run's two parts outside it. (Parley still measures the
/// items up to the end of the view to position them.)
fn for_each_atom_in_view<'a, B: Brush>(
    layout: &'a Layout<B>,
    line: &Line<'a, B>,
    view_start: f32,
    view_end: f32,
    mut visit: impl FnMut(f32, f32),
) {
    let mut items = line.items().peekable();
    while let Some(item) = items.next() {
        let glyph_run = match item {
            PositionedLayoutItem::InlineBox(ibox) => {
                if ibox.x >= view_end {
                    return;
                }
                if ibox.kind == InlineBoxKind::InFlow {
                    visit(ibox.x, ibox.x + ibox.width);
                }
                continue;
            }
            PositionedLayoutItem::GlyphRun(glyph_run) => glyph_run,
        };
        // A run is yielded as one glyph run per style, but its clusters are
        // only reachable through the whole run.
        let run = glyph_run.run();
        // The end of the run's next glyph run, consuming it.
        let mut next_end_in_run = || {
            let next = items.next_if(|item| {
                matches!(item, PositionedLayoutItem::GlyphRun(next)
                    if next.run().index() == run.index())
            });
            match next? {
                PositionedLayoutItem::GlyphRun(next) => Some(next.offset() + next.advance()),
                PositionedLayoutItem::InlineBox(_) => None,
            }
        };
        let run_start = glyph_run.offset();
        if run_start >= view_end {
            return;
        }
        let mut run_end = run_start + glyph_run.advance();

        if run_start < view_start {
            // The run starts out of view: find its end, to skip it or to
            // walk it from there.
            while let Some(end) = next_end_in_run() {
                run_end = end;
            }
            if run_end <= view_start {
                continue;
            }
            let from_end = run_end - view_end < view_start - run_start;
            if from_end && walk_run_from_end(layout, run, run_end, view_start, &mut visit) {
                continue;
            }
        }

        let mut in_view = true;
        walk_clusters(
            run.visual_clusters(),
            !run.is_rtl(),
            run_start,
            1.0,
            |start, end| {
                in_view = start < view_end;
                if in_view {
                    visit(start, end);
                }
                in_view
            },
        );
        if !in_view {
            return;
        }
        while next_end_in_run().is_some() {}
    }
}

/// Visits the atoms of `run`, which ends at `run_end`, from right to left
/// down to `view_start`. Returns `false`, without visiting any, if the run
/// cannot be walked in that direction.
fn walk_run_from_end<'a, B: Brush>(
    layout: &'a Layout<B>,
    run: &Run<'a, B>,
    run_end: f32,
    view_start: f32,
    visit: &mut impl FnMut(f32, f32),
) -> bool {
    let visit = |start, end| {
        visit(start, end);
        start > view_start
    };
    if run.is_rtl() {
        walk_clusters(run.clusters(), true, run_end, -1.0, visit);
        return true;
    }
    // Parley has no public right-to-left iterator for a left-to-right run:
    // step back from its last cluster instead.
    let Some(last_byte) = run.text_range().end.checked_sub(1) else {
        return false;
    };
    let Some(last) = Cluster::from_byte_index(layout, last_byte) else {
        return false;
    };
    let path = last.path();
    let in_run = |cluster: &Cluster<'a, B>| {
        let other = cluster.path();
        other.line_index() == path.line_index() && other.run_index() == path.run_index()
    };
    if path.run_index() != run.index() || last.is_rtl() {
        return false;
    }
    let clusters = std::iter::successors(Some(last), |cluster| {
        cluster.previous_visual().filter(in_run)
    });
    walk_clusters(clusters, false, run_end, -1.0, visit);
    true
}

/// Calls `visit(start, end)` for each atom made of `clusters`, which lie side
/// by side from `origin` in the direction `step` (`1.0` rightwards, `-1.0`
/// leftwards), until it returns `false`. `logical` is whether the clusters
/// are given in logical order rather than reversed.
fn walk_clusters<'a, B: Brush + 'a>(
    clusters: impl Iterator<Item = Cluster<'a, B>>,
    logical: bool,
    origin: f32,
    step: f32,
    mut visit: impl FnMut(f32, f32) -> bool,
) {
    let mut x = origin;
    let mut atom_edge = origin;
    let mut clusters = clusters.peekable();
    while let Some(cluster) = clusters.next() {
        x += step * cluster.advance();
        // Not an atom boundary if the next cluster is part of the same
        // ligature as this one.
        let splits_ligature = match clusters.peek() {
            Some(next) if logical => next.is_ligature_continuation(),
            Some(_) => cluster.is_ligature_continuation(),
            None => false,
        };
        if !splits_ligature {
            if !visit(atom_edge.min(x), atom_edge.max(x)) {
                return;
            }
            atom_edge = x;
        }
    }
}

/// Paint time: the cut for one line given the box's horizontal scroll offset
/// (layout units). `None` when the scrolled view already shows the end of the
/// content, in which case the line is painted whole.
///
/// The marker goes at the inline-end of the scrolled view. As in Gecko, a
/// line whose line box is shortened by floats is limited to the part of that
/// line box that is in view, and is painted whole once none of it is.
///
/// Atoms (see [`for_each_atom_in_view`]) are kept from the inline-start side
/// up to the last one that fits before the marker; everything after it is
/// cut. The first atom in view is always kept (css-overflow §5.1: it is
/// clipped rather than ellipsed), in which case the marker may itself be
/// clipped by the box.
pub fn resolve<B: Brush>(
    overflow: &TextOverflowLayout<B>,
    layout: &Layout<B>,
    truncated: &TruncatedLine,
    line: &Line<'_, B>,
    scroll_x: f32,
) -> Option<Cut> {
    let (line_min, line_max) = line_bounds(line, overflow.max_width);
    let mut visible_start = scroll_x;
    let mut visible_end = scroll_x + overflow.max_width;
    if line_min > EPSILON || line_max < overflow.max_width - EPSILON {
        visible_start = visible_start.max(line_min);
        visible_end = visible_end.min(line_max);
        if visible_end <= visible_start + EPSILON {
            return None;
        }
    }
    let advance = overflow.marker.advance;

    if overflow.is_rtl {
        if truncated.end >= visible_start - 0.5 {
            return None;
        }
        let limit = visible_start + advance;
        // The kept atoms are those on the right, from the leftmost one that
        // fits after the marker. The rightmost atom in view is always kept.
        let mut first_fitting: Option<f32> = None;
        let mut first_in_view: Option<f32> = None;
        for_each_atom_in_view(layout, line, visible_start, visible_end, |start, _| {
            if start >= limit - EPSILON && first_fitting.is_none_or(|x| start < x) {
                first_fitting = Some(start);
            }
            if start < visible_end - EPSILON && first_in_view.is_none_or(|x| start > x) {
                first_in_view = Some(start);
            }
        });
        let first_in_view = first_in_view?;
        let cut_x = first_fitting.map_or(first_in_view, |x| x.min(first_in_view));
        return Some(Cut {
            cut_x,
            marker_x: cut_x - advance,
            is_rtl: true,
        });
    }

    if truncated.end <= visible_end + 0.5 {
        return None;
    }
    let limit = visible_end - advance;
    // The kept atoms are those on the left, up to the rightmost one that fits
    // before the marker. The leftmost atom in view is always kept.
    let mut last_fitting: Option<f32> = None;
    let mut first_in_view: Option<f32> = None;
    for_each_atom_in_view(layout, line, visible_start, visible_end, |_, end| {
        if end <= limit + EPSILON && last_fitting.is_none_or(|x| end > x) {
            last_fitting = Some(end);
        }
        if end > visible_start + EPSILON && first_in_view.is_none_or(|x| end < x) {
            first_in_view = Some(end);
        }
    });
    let first_in_view = first_in_view?;
    let cut_x = last_fitting.map_or(first_in_view, |x| x.max(first_in_view));
    Some(Cut {
        cut_x,
        marker_x: cut_x,
        is_rtl: false,
    })
}
