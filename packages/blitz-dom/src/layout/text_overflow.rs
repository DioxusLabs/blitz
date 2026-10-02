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
//! Only the inline-end edge of left-to-right blocks is handled.
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
    /// Where the line's content ends (`offset + advance`, hanging whitespace
    /// excluded), in layout units: `text-indent` shifts it.
    pub end: f32,
    /// The line's baseline — the block's, not a run's (`vertical-align`).
    pub baseline: f32,
}

/// Every candidate line of an inline context, the box width they are
/// measured against, and the marker they share.
#[derive(Clone, Debug)]
pub struct TextOverflowLayout<B: Brush> {
    pub max_width: f32,
    pub marker: Marker<B>,
    pub lines: Vec<TruncatedLine>,
}

/// Where paint cuts a line for the current scroll position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    /// Content ending past this x (layout units, unscrolled) is not painted.
    /// The marker starts here.
    pub cut_x: f32,
}

impl Cut {
    /// Whether content ending at `end` is hidden by this cut.
    pub fn hides(&self, end: f32) -> bool {
        end > self.cut_x + EPSILON
    }
}

impl<B: Brush> TextOverflowLayout<B> {
    pub fn line(&self, index: usize) -> Option<&TruncatedLine> {
        self.lines.iter().find(|l| l.line_index == index)
    }
}

/// Post-layout: the lines whose content ends past `max_width` (layout units).
pub fn overflowing_lines<B: Brush>(layout: &Layout<B>, max_width: f32) -> Vec<TruncatedLine> {
    let mut lines = Vec::new();
    for (line_index, line) in layout.lines().enumerate() {
        let metrics = line.metrics();
        let end = metrics.offset + metrics.advance - metrics.hanging_advance;
        if end > max_width + 0.5 && line.items().next().is_some() {
            lines.push(TruncatedLine {
                line_index,
                end,
                baseline: metrics.baseline,
            });
        }
    }
    lines
}

/// Paint time: the cut for one line given the box's horizontal scroll offset
/// (layout units). `None` when the scrolled view already shows the end of the
/// content, in which case the line is painted whole.
///
/// The line is walked in visual order as a sequence of atoms: grapheme
/// clusters (a ligature counts as one) and in-flow inline boxes. Atoms are
/// kept until one does not fit before the marker; that atom and everything
/// after it is cut. The first atom in view is always kept (css-overflow §5.1:
/// it is clipped rather than ellipsed), in which case the marker may itself
/// be clipped by the box.
pub fn resolve<B: Brush>(
    layout: &TextOverflowLayout<B>,
    truncated: &TruncatedLine,
    line: &Line<'_, B>,
    scroll_x: f32,
) -> Option<Cut> {
    let visible_end = scroll_x + layout.max_width;
    if truncated.end <= visible_end + 0.5 {
        return None;
    }
    let limit = visible_end - layout.marker.advance;

    // End of the last atom that is kept.
    let mut cut_x: Option<f32> = None;
    // Whether a kept atom is (at least partly) inside the scrolled view.
    let mut kept_in_view = false;
    // Visits one atom; returns `false` once the cut has been found.
    let mut visit = |end: f32| -> bool {
        if end > limit + EPSILON && kept_in_view {
            return false;
        }
        kept_in_view |= end > scroll_x + EPSILON;
        cut_x = Some(end);
        true
    };

    // A run is yielded as one glyph run per style, but its clusters are only
    // reachable through the whole run: walk them once, at the run's first
    // (visually leftmost) glyph run.
    let mut last_run_index = None;
    'items: for item in line.items() {
        match item {
            PositionedLayoutItem::InlineBox(ibox) => {
                last_run_index = None;
                if ibox.kind == InlineBoxKind::InFlow && !visit(ibox.x + ibox.width) {
                    break 'items;
                }
            }
            PositionedLayoutItem::GlyphRun(glyph_run) => {
                let run = glyph_run.run();
                if last_run_index.replace(run.index()) == Some(run.index()) {
                    continue;
                }
                let is_rtl = run.is_rtl();
                let mut x = glyph_run.offset();
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
                    if !splits_ligature && !visit(x) {
                        break 'items;
                    }
                }
            }
        }
    }

    cut_x.map(|cut_x| Cut { cut_x })
}
