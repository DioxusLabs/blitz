//! `text-overflow`: which lines of an inline context are too wide for their
//! box, where to cut them, and the shaped marker (`…` or a string) to draw at
//! the cut. Computed right after Parley has laid the lines out, stored on the
//! [`TextLayout`], consumed by the painter.
//!
//! Kept separate from layout and paint so it can move into Parley later:
//! nothing here depends on the DOM, only on a `parley::Layout` and the
//! marker requested by style.
//!
//! [`TextLayout`]: crate::node::TextLayout

use parley::{FontData, Layout, PositionedLayoutItem};
use skrifa::MetadataProvider as _;
use skrifa::raw::FontRef;
use style::values::specified::text::TextOverflowSide;

use crate::node::TextBrush;

/// The marker to draw on one truncated line, already shaped in the font of
/// that line's first run so it matches its size and style.
#[derive(Clone, Debug)]
pub struct Marker {
    pub font: FontData,
    pub font_size: f32,
    pub brush: TextBrush,
    /// Glyph ids and advances, in layout units.
    pub glyphs: Vec<(u32, f32)>,
    pub advance: f32,
}

/// A line that overflows its box: glyphs whose end is past `cut_x` are not
/// painted, the marker is drawn starting at `marker_x` on the baseline.
#[derive(Clone, Debug)]
pub struct TruncatedLine {
    pub line_index: usize,
    pub cut_x: f32,
    pub marker_x: f32,
    pub baseline: f32,
    pub marker: Marker,
}

/// Every truncated line of an inline context.
#[derive(Clone, Debug, Default)]
pub struct TextOverflowLayout {
    pub lines: Vec<TruncatedLine>,
}

impl TextOverflowLayout {
    pub fn line(&self, index: usize) -> Option<&TruncatedLine> {
        self.lines.iter().find(|l| l.line_index == index)
    }
}

/// Shapes `marker` in `font` at `font_size`: glyph ids with their advances.
/// `U+2026` falls back to three periods when the font lacks it.
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

/// Finds the lines wider than `max_width` (layout units) and decides, for
/// each, the cut and the marker. `side` is the inline-end `text-overflow`
/// value; `Clip` yields `None`.
pub fn compute(
    layout: &Layout<TextBrush>,
    side: &TextOverflowSide,
    max_width: f32,
) -> Option<Box<TextOverflowLayout>> {
    if matches!(side, TextOverflowSide::Clip) {
        return None;
    }
    let mut out = TextOverflowLayout::default();
    for (index, line) in layout.lines().enumerate() {
        if line.metrics().advance <= max_width + 0.5 {
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
        let Some((glyphs, advance)) = shape_marker(&font, font_size, side) else {
            continue;
        };
        let cut_x = (max_width - advance).max(0.0);
        // The marker starts right after the last glyph that fits, so it is
        // never separated from the text by a gap.
        let mut marker_x = 0.0f32;
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                for g in run.positioned_glyphs() {
                    if g.x + g.advance <= cut_x + 0.01 {
                        marker_x = marker_x.max(g.x + g.advance);
                    }
                }
            }
        }
        out.lines.push(TruncatedLine {
            line_index: index,
            cut_x,
            marker_x,
            baseline: first.baseline(),
            marker: Marker {
                font,
                font_size,
                brush: first.style().brush,
                glyphs,
                advance,
            },
        });
    }
    (!out.lines.is_empty()).then(|| Box::new(out))
}
