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

use parley::{Brush, FontData, InlineBoxKind, Layout, Line, PositionedLayoutItem};

/// Slack for float rounding when comparing positions (layout units).
const EPSILON: f32 = 0.01;

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
                    font: run.run().font().font.clone(),
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

/// Post-layout: whether the paragraph is right-to-left, and the lines whose
/// content ends outside their line box (see [`line_bounds`], layout units) on
/// the inline-end side.
pub fn overflowing_lines<B: Brush>(
    layout: &Layout<B>,
    max_width: f32,
) -> (bool, Vec<TruncatedLine>) {
    let is_rtl = layout.is_rtl();
    let mut lines = Vec::new();
    for (line_index, line) in layout.lines().enumerate() {
        let metrics = line.metrics();
        let (min, max) = line_bounds(&line, max_width);
        let left = metrics.inline_min_coord + metrics.offset;
        // Hanging whitespace is at the inline-end of the line.
        let (end, overflows) = if is_rtl {
            let end = left + metrics.hanging_advance;
            (end, end < min - 0.5)
        } else {
            let end = left + metrics.advance - metrics.hanging_advance;
            (end, end > max + 0.5)
        };
        if overflows && line.items().next().is_some() {
            lines.push(TruncatedLine {
                line_index,
                end,
                baseline: metrics.baseline,
            });
        }
    }
    (is_rtl, lines)
}

/// Calls `visit(start, end)` for each atom of the line, left to right, until
/// it returns `false`. Atoms are grapheme clusters (a ligature counts as one)
/// and in-flow inline boxes.
fn for_each_atom<B: Brush>(line: &Line<'_, B>, mut visit: impl FnMut(f32, f32) -> bool) {
    // A run is yielded as one glyph run per style, but its clusters are only
    // reachable through the whole run: walk them once, at the run's first
    // (visually leftmost) glyph run.
    let mut last_run_index = None;
    for item in line.items() {
        match item {
            PositionedLayoutItem::InlineBox(ibox) => {
                last_run_index = None;
                if ibox.kind == InlineBoxKind::InFlow && !visit(ibox.x, ibox.x + ibox.width) {
                    return;
                }
            }
            PositionedLayoutItem::GlyphRun(glyph_run) => {
                let run = glyph_run.run();
                if last_run_index.replace(run.index()) == Some(run.index()) {
                    continue;
                }
                let is_rtl = run.is_rtl();
                let mut x = glyph_run.offset();
                let mut atom_start = x;
                let mut clusters = run.visual_clusters().peekable();
                while let Some(cluster) = clusters.next() {
                    x += cluster.advance();
                    // Not an atom boundary if the next cluster (visually) is
                    // part of the same ligature as this one.
                    let splits_ligature = match clusters.peek() {
                        Some(_) if is_rtl => cluster.is_ligature_continuation(),
                        Some(next) => next.is_ligature_continuation(),
                        None => false,
                    };
                    if !splits_ligature {
                        if !visit(atom_start, x) {
                            return;
                        }
                        atom_start = x;
                    }
                }
            }
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
/// Starting from the inline-start side, atoms (see [`for_each_atom`]) are kept
/// until one does not fit before the marker; that atom and everything after it
/// is cut. The first atom in view is always kept (css-overflow §5.1: it is
/// clipped rather than ellipsed), in which case the marker may itself be
/// clipped by the box.
pub fn resolve<B: Brush>(
    layout: &TextOverflowLayout<B>,
    truncated: &TruncatedLine,
    line: &Line<'_, B>,
    scroll_x: f32,
) -> Option<Cut> {
    let (line_min, line_max) = line_bounds(line, layout.max_width);
    let mut visible_start = scroll_x;
    let mut visible_end = scroll_x + layout.max_width;
    if line_min > EPSILON || line_max < layout.max_width - EPSILON {
        visible_start = visible_start.max(line_min);
        visible_end = visible_end.min(line_max);
        if visible_end <= visible_start + EPSILON {
            return None;
        }
    }
    let advance = layout.marker.advance;

    if layout.is_rtl {
        if truncated.end >= visible_start - 0.5 {
            return None;
        }
        let limit = visible_start + advance;
        // The line is walked left to right, i.e. from the inline-end side, so
        // the kept atoms are a suffix: from the first atom that fits after the
        // marker, or from the last atom in view if that is further left.
        let mut first_fitting: Option<f32> = None;
        let mut last_in_view: Option<f32> = None;
        for_each_atom(line, |start, _| {
            if start >= limit - EPSILON && first_fitting.is_none() {
                first_fitting = Some(start);
            }
            if start < visible_end - EPSILON {
                last_in_view = Some(start);
            }
            true
        });
        let cut_x = match (first_fitting, last_in_view) {
            (Some(a), Some(b)) => a.min(b),
            (a, b) => a.or(b)?,
        };
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
    // End of the last atom that is kept.
    let mut cut_x: Option<f32> = None;
    // Whether a kept atom is (at least partly) inside the scrolled view.
    let mut kept_in_view = false;
    for_each_atom(line, |_, end| {
        if end > limit + EPSILON && kept_in_view {
            return false;
        }
        kept_in_view |= end > visible_start + EPSILON;
        cut_x = Some(end);
        true
    });

    cut_x.map(|cut_x| Cut {
        cut_x,
        marker_x: cut_x,
        is_rtl: false,
    })
}
