//! `text-overflow`: which lines of an inline context are too wide for their
//! box, and the shaped marker (`…` or a string) to draw at the cut.
//!
//! Split in two so that scrolling stays cheap:
//! - **post-layout** ([`compute`], called from `inline.rs` right after the
//!   lines are final): which lines overflow and where their content ends,
//!   plus the marker shaped once from the block container's style (font,
//!   size, colour — css-overflow §5.2). Stored on the [`TextLayout`] as
//!   `Option<Box<TextOverflowLayout>>`.
//! - **at paint time** ([`resolve`]): the cut position (on a grapheme-cluster
//!   boundary) for the current horizontal scroll offset, and whether the
//!   marker is needed at all — a scrolled box whose content end is in view
//!   shows no marker (css-overflow §5, "ellipsis-scrolling").
//!
//! DOM-free on purpose, so it can move into Parley later.
//!
//! [`TextLayout`]: crate::node::TextLayout

use parley::{FontData, Layout, Line, PositionedLayoutItem};
use style::properties::ComputedValues;
use style::values::computed::Overflow;
use style::values::specified::box_::DisplayInside;
use style::values::specified::text::TextOverflowSide;

use crate::node::TextBrush;

/// One shaped run of the marker (Parley may split it across fonts when the
/// block's font lacks a glyph).
#[derive(Clone, Debug)]
pub struct MarkerRun {
    pub font: FontData,
    pub font_size: f32,
    pub normalized_coords: Vec<i16>,
    /// Glyph ids with their x offset within the marker, in layout units.
    pub glyphs: Vec<(u32, f32)>,
}

/// The marker, shaped once per inline context in the **block container's**
/// style (css-overflow §5.2: styled and baseline-aligned according to the
/// block), so a small block cuts a large span with a small marker.
#[derive(Clone, Debug)]
pub struct Marker {
    pub runs: Vec<MarkerRun>,
    pub advance: f32,
    pub brush: TextBrush,
}

impl Marker {
    /// Flattens a shaped marker layout (one line) into runs.
    pub fn from_layout(layout: &Layout<TextBrush>, brush: TextBrush) -> Option<Self> {
        let line = layout.lines().next()?;
        let mut runs = Vec::new();
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                let glyphs = run.positioned_glyphs().map(|g| (g.id, g.x)).collect();
                runs.push(MarkerRun {
                    font: run.run().font().clone(),
                    font_size: run.run().font_size(),
                    normalized_coords: run.run().normalized_coords().to_vec(),
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
    /// Where the line's content ends (`offset + advance`, trailing whitespace
    /// excluded), in layout units: `text-indent` shifts it.
    pub end: f32,
    /// The line's baseline — the block's, not a run's (`vertical-align`).
    pub baseline: f32,
}

/// Every candidate line of an inline context, the box width they are
/// measured against, and the marker they share.
#[derive(Clone, Debug)]
pub struct TextOverflowLayout {
    pub max_width: f32,
    pub marker: Marker,
    pub lines: Vec<TruncatedLine>,
}

/// Where paint cuts a line for the current scroll position.
#[derive(Clone, Copy, Debug)]
pub struct Cut {
    /// Glyphs whose end is past this x (layout units, unscrolled) are not painted.
    pub cut_x: f32,
    /// Where the marker starts: right after the last glyph that fits.
    pub marker_x: f32,
}

impl TextOverflowLayout {
    pub fn line(&self, index: usize) -> Option<&TruncatedLine> {
        self.lines.iter().find(|l| l.line_index == index)
    }
}

/// The inline-end `text-overflow` value of a block container that clips its
/// overflow; `None` otherwise (`text-overflow: clip`, or no clipping).
pub fn side_for(styles: &ComputedValues) -> Option<TextOverflowSide> {
    if matches!(styles.get_box().overflow_x, Overflow::Visible) {
        return None;
    }
    // Stylo stores a single value as `(Clip, value)` with `sides_are_logical`,
    // and two values as `(start, end)`: either way `second` is the inline-end
    // side, the right edge in Blitz's left-to-right inline layout.
    match &styles.get_text().text_overflow.second {
        TextOverflowSide::Clip => None,
        side => Some(side.clone()),
    }
}

/// Whether `styles` describe a block container (`text-overflow` applies to
/// block containers only, not to the anonymous wrappers of flex/grid items).
pub fn is_block_container(styles: &ComputedValues) -> bool {
    matches!(
        styles.clone_display().inside(),
        DisplayInside::Flow | DisplayInside::FlowRoot
    )
}

/// Post-layout: the lines whose content ends past `max_width` (layout
/// units), with the marker shaped from the block's style.
pub fn compute(
    layout: &Layout<TextBrush>,
    marker: Marker,
    max_width: f32,
) -> Option<Box<TextOverflowLayout>> {
    let mut lines = Vec::new();
    for (index, line) in layout.lines().enumerate() {
        let metrics = line.metrics();
        let end = metrics.offset + metrics.advance - metrics.trailing_whitespace;
        if end <= max_width + 0.5 {
            continue;
        }
        // Lines made only of inline boxes have nothing to cut at glyph level.
        let has_glyphs = line
            .items()
            .any(|item| matches!(item, PositionedLayoutItem::GlyphRun(_)));
        if !has_glyphs {
            continue;
        }
        lines.push(TruncatedLine {
            line_index: index,
            end,
            baseline: metrics.baseline,
        });
    }
    (!lines.is_empty()).then(|| {
        Box::new(TextOverflowLayout {
            max_width,
            marker,
            lines,
        })
    })
}

/// Paint time: the cut for one line given the box's horizontal scroll offset
/// (layout units). `None` when the scrolled view already shows the end of the
/// content, in which case the line is painted whole. The cut falls on a
/// grapheme-cluster boundary: whole characters are elided, never half a
/// base+combining pair.
pub fn resolve(
    layout: &TextOverflowLayout,
    truncated: &TruncatedLine,
    line: &Line<'_, TextBrush>,
    scroll_x: f32,
) -> Option<Cut> {
    let visible_end = scroll_x + layout.max_width;
    if truncated.end <= visible_end + 0.5 {
        return None;
    }
    let limit = (visible_end - layout.marker.advance).max(scroll_x);
    // Last cluster boundary that fits before the marker.
    let mut cut_x = scroll_x;
    for item in line.items() {
        if let PositionedLayoutItem::GlyphRun(run) = item {
            let mut x = run.offset();
            for cluster in run.run().clusters() {
                let end = x + cluster.advance();
                if end <= limit + 0.01 {
                    cut_x = cut_x.max(end);
                }
                x = end;
            }
        }
    }
    Some(Cut {
        cut_x,
        marker_x: cut_x,
    })
}
