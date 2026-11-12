//! Painting text: the scratch state and helpers every text backend shares, and a module for each
//! backend that paints its own layouts. The decorations and shadows they share are in
//! [`crate::text_decorations`].

use anyrender::PaintScene;
use blitz_dom::NodeId;
use blitz_dom::node::TextLayout;
use blitz_dom::text::InlineText as _;
use kurbo::Affine;
use peniko::Fill;
use std::sync::Arc;

use crate::SELECTION_COLOR;
use crate::color::Color;
use crate::text_decorations::{
    DecorationStackEntry, LineDecoration, ResolvedTextShadow, WinAscentCache,
};

blitz_dom::cfg_text_backend! {
    parley => {
        mod parley;
        pub(crate) use self::parley::{
            paint_inline_layout, paint_inline_text, paint_marker, paint_text_input,
        };
    }
}

/// Paints the highlight of the selected text between byte offsets `start` and `end` of an inline
/// formatting context.
///
/// `transform` takes the content box's device pixels, moved down by the layout's block offset,
/// onto the scene.
fn paint_selection(
    scene: &mut impl PaintScene,
    layout: &TextLayout,
    transform: Affine,
    start: usize,
    end: usize,
) {
    layout.for_each_selection_rect(start, end, |rect| {
        scene.fill(Fill::NonZero, transform, SELECTION_COLOR, None, &rect);
    });
}

/// Per glyph run values recorded while walking a line, so the line can be painted in
/// passes (shadows beneath, then glyphs) without re-resolving styles.
pub(crate) struct RunRecord {
    pub(crate) fill_color: Color,
    pub(crate) shadows: Option<Arc<[ResolvedTextShadow]>>,
}

/// Reusable scratch storage for drawing text across all inline formatting contexts in a
/// document. The vectors are cleared between uses without releasing their allocations.
#[derive(Default)]
pub(crate) struct DrawTextContext {
    pub(super) stack: Vec<DecorationStackEntry>,
    pub(super) path_scratch: Vec<NodeId>,
    pub(super) deco_boxes: Vec<LineDecoration>,
    pub(super) run_records: Vec<RunRecord>,
    pub(super) win_ascent_ratios: WinAscentCache,
}

#[derive(Clone, Copy)]
pub(crate) struct TextPaintOptions {
    pub scale: f64,
    pub brush_override: Option<Color>,
}

/// Approximates macOS font smoothing (stem dilation). CoreGraphics dilates each side by an amount
/// in device pixels that grows with the device ppem up to 0.3px (fitted to Safari's ink at 9-72px
/// and 1x-3x scale), and keeps dilating at large sizes, so there is no ppem cutoff. glifo applies
/// the embolden in user units, so the device amounts are divided by the transform's scale.
pub(super) fn font_embolden(font_size: f64, color: Color, transform: Affine) -> kurbo::Vec2 {
    const MAX_DEVICE_EMBOLDEN: f64 = 0.3;
    let [a, b, c, d, _, _] = transform.as_coeffs();
    let (scale_x, scale_y) = (a.hypot(b), c.hypot(d));
    let device_ppem = font_size * (a * d - b * c).abs().sqrt();
    if scale_x == 0.0 || scale_y == 0.0 || !device_ppem.is_finite() {
        return kurbo::Vec2::default();
    }
    let fs = device_ppem * embolden_scale(color);
    kurbo::Vec2::new(
        (0.0121 * fs).min(MAX_DEVICE_EMBOLDEN) / scale_x,
        (0.015125 * fs).min(MAX_DEVICE_EMBOLDEN) / scale_y,
    )
}

/// Scales Pathfinder's stem-darkening factors by the text's relative luminance. CoreText
/// dilates light text more than dark text; the points are fitted to Safari's ink for text
/// colours from black to white on black and white backgrounds.
fn embolden_scale(color: Color) -> f64 {
    const POINTS: [(f64, f64); 4] = [(0.1, 0.5), (0.125, 0.72), (0.35, 0.72), (1.0, 1.3)];
    let lum = color.discard_alpha().relative_luminance() as f64;
    let mut prev = POINTS[0];
    if lum <= prev.0 {
        return prev.1;
    }
    for point in &POINTS[1..] {
        if lum <= point.0 {
            return prev.1 + (point.1 - prev.1) * (lum - prev.0) / (point.0 - prev.0);
        }
        prev = *point;
    }
    prev.1
}
