//! `text-overflow`: which lines of an inline context are too wide for their
//! box, and the shaped marker (`…` or a string) to draw at the cut.
//!
//! Split in two so that scrolling stays cheap:
//! - **post-layout** ([`compute`], called from `inline.rs` right after the
//!   lines are final): which lines overflow, their content advance, and the
//!   marker shaped in the font of each line's first run. Stored on the
//!   [`TextLayout`] as `Option<Box<TextOverflowLayout>>`.
//! - **at paint time** ([`resolve`]): the cut position for the current
//!   horizontal scroll offset, and whether the marker is needed at all — a
//!   scrolled box whose content end is in view shows no marker
//!   (css-overflow §5, "ellipsis-scrolling").
//!
//! DOM-free on purpose, so it can move into Parley later.
//!
//! [`TextLayout`]: crate::node::TextLayout

use parley::{FontData, Layout, Line, PositionedLayoutItem};
use skrifa::MetadataProvider as _;
use skrifa::raw::FontRef;
use style::properties::ComputedValues;
use style::values::computed::Overflow;
use style::values::specified::box_::DisplayInside;
use style::values::specified::text::TextOverflowSide;

use crate::node::TextBrush;

/// The marker of one truncated line, shaped in the font of that line's first
/// run so it matches its size, style and variation settings.
#[derive(Clone, Debug)]
pub struct Marker {
    pub font: FontData,
    pub font_size: f32,
    pub normalized_coords: Vec<i16>,
    pub brush: TextBrush,
    /// Glyph ids and advances, in layout units.
    pub glyphs: Vec<(u32, f32)>,
    pub advance: f32,
}

/// A line whose content is wider than the box.
#[derive(Clone, Debug)]
pub struct TruncatedLine {
    pub line_index: usize,
    /// Advance of the line's content, trailing whitespace excluded.
    pub advance: f32,
    pub baseline: f32,
    pub marker: Marker,
}

/// Every candidate line of an inline context, with the box width they are
/// measured against.
#[derive(Clone, Debug)]
pub struct TextOverflowLayout {
    pub max_width: f32,
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

/// Shapes the marker in `font` at `font_size`; `U+2026` falls back to three
/// periods when the font lacks it.
fn shape_marker(
    font: &FontData,
    font_size: f32,
    side: &TextOverflowSide,
) -> Option<(Vec<(u32, f32)>, f32)> {
    let font_ref = FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let charmap = font_ref.charmap();
    let metrics = font_ref.glyph_metrics(
        skrifa::instance::Size::new(font_size),
        skrifa::instance::LocationRef::default(),
    );
    let shape = |text: &mut dyn Iterator<Item = char>| -> Option<(Vec<(u32, f32)>, f32)> {
        let mut glyphs = Vec::new();
        let mut advance = 0.0f32;
        for ch in text {
            let gid = charmap.map(ch)?;
            let adv = metrics.advance_width(gid).unwrap_or(0.0);
            glyphs.push((gid.to_u32(), adv));
            advance += adv;
        }
        Some((glyphs, advance))
    };
    match side {
        TextOverflowSide::Clip => None,
        TextOverflowSide::Ellipsis => {
            shape(&mut std::iter::once('\u{2026}')).or_else(|| shape(&mut "...".chars()))
        }
        TextOverflowSide::String(s) => shape(&mut s.as_ref().chars()),
    }
}

/// Post-layout: the lines wider than `max_width` (layout units) with their
/// shaped marker. Lines without a glyph run (only inline boxes) are skipped:
/// there is no run to take a font from (see Limitations in the PR).
pub fn compute(
    layout: &Layout<TextBrush>,
    side: &TextOverflowSide,
    max_width: f32,
) -> Option<Box<TextOverflowLayout>> {
    if matches!(side, TextOverflowSide::Clip) {
        return None;
    }
    let mut lines = Vec::new();
    for (index, line) in layout.lines().enumerate() {
        let metrics = line.metrics();
        let advance = metrics.advance - metrics.trailing_whitespace;
        if advance <= max_width + 0.5 {
            continue;
        }
        let Some(first) = line.items().find_map(|item| match item {
            PositionedLayoutItem::GlyphRun(run) => Some(run),
            _ => None,
        }) else {
            continue;
        };
        let font = first.run().font().clone();
        let font_size = first.run().font_size();
        let Some((glyphs, marker_advance)) = shape_marker(&font, font_size, side) else {
            continue;
        };
        lines.push(TruncatedLine {
            line_index: index,
            advance,
            baseline: first.baseline(),
            marker: Marker {
                font,
                font_size,
                normalized_coords: first.run().normalized_coords().to_vec(),
                brush: first.style().brush,
                glyphs,
                advance: marker_advance,
            },
        });
    }
    (!lines.is_empty()).then(|| Box::new(TextOverflowLayout { max_width, lines }))
}

/// Paint time: the cut for one line given the box's horizontal scroll offset
/// (layout units). `None` when the scrolled view already shows the end of the
/// content, in which case the line is painted whole.
pub fn resolve(
    layout: &TextOverflowLayout,
    truncated: &TruncatedLine,
    line: &Line<'_, TextBrush>,
    scroll_x: f32,
) -> Option<Cut> {
    let visible_end = scroll_x + layout.max_width;
    if truncated.advance <= visible_end + 0.5 {
        return None;
    }
    let cut_x = (visible_end - truncated.marker.advance).max(scroll_x);
    // The marker starts right after the last glyph that fits, so it is never
    // separated from the text by a gap.
    let mut marker_x = scroll_x;
    for item in line.items() {
        if let PositionedLayoutItem::GlyphRun(run) = item {
            for g in run.positioned_glyphs() {
                let end = g.x + g.advance;
                if end <= cut_x + 0.01 {
                    marker_x = marker_x.max(end);
                }
            }
        }
    }
    Some(Cut { cut_x, marker_x })
}
