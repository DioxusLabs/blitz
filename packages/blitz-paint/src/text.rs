use anyrender::PaintScene;
use blitz_dom::{BaseDocument, NodeId, node::TextBrush, util::ToColorColor};
use kurbo::{Affine, BezPath, Cap, Circle, Rect, Stroke};
use parley::{Affinity, Cursor, Layout, Line, PositionedLayoutItem, Selection};
use peniko::Fill;
use style::properties::generated::longhands::text_decoration_style::computed_value::T as TextDecorationStyle;
use style::values::computed::{
    Length, TextDecorationLength, TextDecorationLine, TextUnderlinePosition,
};
use style::values::generics::text::{GenericTextDecorationInset, GenericTextDecorationLength};

use crate::color::{Color, ToColorColor as _};
use crate::{FONT_EMBOLDEN_ENABLED, SELECTION_COLOR};

/// Draw the backgrounds of inline elements (e.g. `<span style="background: ...">`).
///
/// Each glyph run carries the node id of the innermost inline element it belongs to
/// (via its brush). We look up that node's `background-color` and, if non-transparent,
/// fill a rectangle covering the run's advance and its font's ascent/descent so that the
/// background sits behind the text.
///
/// The inline root's own background is painted separately (as a normal block box), so
/// runs belonging to the root are skipped to avoid drawing it twice.
pub(crate) fn draw_inline_backgrounds<'a>(
    scene: &mut impl PaintScene,
    lines: impl Iterator<Item = Line<'a, TextBrush>>,
    doc: &BaseDocument,
    transform: Affine,
    inline_root_id: NodeId,
) {
    for line in lines {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };

            let node_id = glyph_run.style().brush.id;
            if node_id == inline_root_id {
                continue;
            }

            let Some(styles) = doc.get_node(node_id).and_then(|node| node.primary_styles()) else {
                continue;
            };

            let current_color = styles.clone_color();
            let bg_color = styles
                .get_background()
                .background_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            if bg_color == Color::TRANSPARENT {
                continue;
            }

            let metrics = glyph_run.run().metrics();
            let x = glyph_run.offset() as f64;
            let w = glyph_run.advance() as f64;
            let baseline = glyph_run.baseline() as f64;
            let y0 = baseline - metrics.ascent as f64;
            let y1 = baseline + metrics.descent as f64;
            let rect = Rect::new(x, y0, x + w, y1);

            scene.fill(Fill::NonZero, transform, bg_color, None, &rect);
        }
    }
}

/// The font's OS/2 `usWinAscent`, in the same (device) pixels as `font_size`.
///
/// This is the ascent browsers use as the top of the "em box" when positioning overlines. It
/// is typically taller than the hhea ascent Parley exposes via [`parley::layout::run::RunMetrics`].
/// Returns `None` when the font has no OS/2 table.
fn win_ascent(font: &parley::FontData, font_size: f32) -> Option<f32> {
    use read_fonts::{FontRef, TableProvider as _};
    let font_ref = FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let units_per_em = font_ref.head().ok()?.units_per_em();
    let win_ascent = font_ref.os2().ok()?.us_win_ascent();
    Some(win_ascent as f32 * font_size / units_per_em as f32)
}

/// Mirrors Blink's `SelectBestDashGap`: choose the gap length (as close as possible to
/// `gap_length`) that fits a whole number of `dash_length` dashes across `stroke_length`,
/// so dashes are evenly distributed and a dash lands at each end of the line.
fn select_best_dash_gap(stroke_length: f64, dash_length: f64, gap_length: f64) -> f64 {
    let available_length = stroke_length + gap_length;
    let min_num_dashes = (available_length / (dash_length + gap_length)).floor();
    let max_num_dashes = min_num_dashes + 1.0;
    let min_num_gaps = min_num_dashes - 1.0;
    let max_num_gaps = max_num_dashes - 1.0;
    if min_num_gaps < 1.0 {
        return gap_length;
    }
    let min_gap = (stroke_length - min_num_dashes * dash_length) / min_num_gaps;
    let max_gap = (stroke_length - max_num_dashes * dash_length) / max_num_gaps;
    if max_gap <= 0.0 || (min_gap - gap_length).abs() < (max_gap - gap_length).abs() {
        min_gap
    } else {
        max_gap
    }
}

pub(crate) fn stroke_text<'a>(
    scene: &mut impl PaintScene,
    lines: impl Iterator<Item = Line<'a, TextBrush>>,
    doc: &BaseDocument,
    transform: Affine,
    scale: f64,
) {
    for line in lines {
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                let run = glyph_run.run();
                let font = run.font();
                let font_size = run.font_size();
                let metrics = run.metrics();
                let style = glyph_run.style();
                let synthesis = run.synthesis();
                let glyph_xform = synthesis
                    .skew()
                    .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));

                // Styles
                let styles = doc
                    .get_node(style.brush.id)
                    .unwrap()
                    .primary_styles()
                    .unwrap();
                let itext_styles = styles.get_inherited_text();
                let text_styles = styles.get_text();
                let text_color = itext_styles.color.as_color_color();
                let text_decoration_color = text_styles
                    .text_decoration_color
                    .as_absolute()
                    .map(ToColorColor::as_color_color)
                    .unwrap_or(text_color);
                let text_decoration_brush = anyrender::Paint::from(text_decoration_color);
                let text_decoration_line = text_styles.text_decoration_line;
                let text_decoration_style = text_styles.text_decoration_style;
                let text_decoration_thickness: TextDecorationLength =
                    text_styles.text_decoration_thickness.clone();

                // `text-decoration-inset` shortens (or, when negative, extends) the decoration
                // line from the inline-start and inline-end edges of the text. Resolve the
                // start/end lengths to device pixels; percentages are resolved against the
                // decoration line length (the glyph run's advance). `auto` is treated as no inset.
                let (inset_start, inset_end) = match &text_styles.text_decoration_inset {
                    GenericTextDecorationInset::LengthPercentage { start, end } => {
                        // The advance is already in device pixels; convert to CSS pixels so the
                        // resolved (device-pixel) result can be scaled back consistently.
                        let line_length = Length::new(glyph_run.advance() / scale as f32);
                        (
                            start.resolve(line_length).px() as f64 * scale,
                            end.resolve(line_length).px() as f64 * scale,
                        )
                    }
                    GenericTextDecorationInset::Auto => (0.0, 0.0),
                };
                let has_underline = text_decoration_line.contains(TextDecorationLine::UNDERLINE);
                let has_overline = text_decoration_line.contains(TextDecorationLine::OVERLINE);
                let has_strikethrough =
                    text_decoration_line.contains(TextDecorationLine::LINE_THROUGH);

                let embolden = if FONT_EMBOLDEN_ENABLED {
                    let fs = font_size as f64 / scale;
                    kurbo::Vec2::new((0.015125 * fs).min(0.3), (0.0121 * fs).min(0.3))
                } else {
                    kurbo::Vec2::default()
                };

                scene.draw_glyphs(
                    font,
                    font_size,
                    !FONT_EMBOLDEN_ENABLED, // hint
                    run.normalized_coords(),
                    embolden,
                    Fill::NonZero,
                    &anyrender::Paint::from(text_color),
                    1.0, // alpha
                    transform,
                    glyph_xform,
                    glyph_run.positioned_glyphs().map(|glyph| anyrender::Glyph {
                        id: glyph.id as _,
                        x: glyph.x,
                        y: glyph.y,
                    }),
                );

                // Draws a single decoration line of the given `text-decoration-style`. `y` is
                // the centre of the line; `x`/`w` span the glyph run. Geometry is built
                // explicitly (rather than relying on backend dash support) so it renders
                // consistently across renderers.
                //
                // `double_dir` points away from the text (`+1` = down, `-1` = up) and is used
                // to place the second line of a `double` decoration.
                let mut draw_decoration_line =
                    |offset: f32, size: f32, brush: &anyrender::Paint, double_dir: f64| {
                        // Inset the line from the run's start/end edges (assumes LTR: start is
                        // the left edge). Negative insets extend the line past the text.
                        let x = glyph_run.offset() as f64 + inset_start;
                        let w = glyph_run.advance() as f64 - inset_start - inset_end;
                        if w <= 0.0 {
                            return;
                        }
                        let y = (glyph_run.baseline() - offset + size / 2.0) as f64;
                        let size = size as f64;
                        let butt_stroke = Stroke::new(size).with_caps(Cap::Butt);

                        match text_decoration_style {
                            TextDecorationStyle::MozNone => {
                                // no line. Equivalent to `text-decoration-line: none`
                            }
                            // `solid`
                            TextDecorationStyle::Solid => {
                                let line = kurbo::Line::new((x, y), (x + w, y));
                                scene.stroke(&butt_stroke, transform, brush, None, &line);
                            }
                            // Two lines, each `size` thick, separated by a 1px (CSS) gap. This
                            // matches Chrome, where the second line is offset by `thickness + 1px`
                            // and sits further from the text (below for underline, above for
                            // overline).
                            TextDecorationStyle::Double => {
                                let one_css_px = scale;
                                for cy in [y, y + double_dir * (size + one_css_px)] {
                                    let line = kurbo::Line::new((x, cy), (x + w, cy));
                                    scene.stroke(&butt_stroke, transform, brush, None, &line);
                                }
                            }
                            // Round dots (diameter = line thickness) spaced one dot apart.
                            TextDecorationStyle::Dotted => {
                                let radius = size / 2.0;
                                let step = 2.0 * size;
                                let mut cx = x + radius;
                                while cx <= x + w - radius {
                                    let dot = Circle::new((cx, y), radius);
                                    scene.fill(Fill::NonZero, transform, brush, None, &dot);
                                    cx += step;
                                }
                            }
                            // Dashes as filled rectangles. Dash/gap lengths are relative to the
                            // thickness (matching Blink): thinner lines use proportionally longer
                            // dashes and gaps so they don't read as dotted or solid.
                            TextDecorationStyle::Dashed => {
                                let (dash_ratio, gap_ratio) =
                                    if size >= 3.0 { (2.0, 1.0) } else { (3.0, 2.0) };
                                let dash = size * dash_ratio;
                                let nominal_gap = size * gap_ratio;
                                // Nudge the gap so a whole number of dashes fits the run evenly.
                                let gap = if w > 2.0 * dash {
                                    select_best_dash_gap(w, dash, nominal_gap)
                                } else {
                                    nominal_gap
                                };
                                let mut dx = x;
                                while dx < x + w {
                                    let end = (dx + dash).min(x + w);
                                    let rect = Rect::new(dx, y - size / 2.0, end, y + size / 2.0);
                                    scene.fill(Fill::NonZero, transform, brush, None, &rect);
                                    dx += dash + gap;
                                }
                            }
                            // A squiggle built from alternating quadratic Béziers.
                            TextDecorationStyle::Wavy => {
                                let amplitude = size;
                                let half_wave = (2.0 * size).max(1.0);
                                let mut path = BezPath::new();
                                path.move_to((x, y));
                                let mut px = x;
                                let mut up = true;
                                while px < x + w {
                                    let nx = (px + half_wave).min(x + w);
                                    // A quad Bézier reaches half the control-point offset at its
                                    // midpoint, so use `2 * amplitude` to hit the target peak.
                                    let ctrl_y = if up {
                                        y - 2.0 * amplitude
                                    } else {
                                        y + 2.0 * amplitude
                                    };
                                    path.quad_to(((px + nx) / 2.0, ctrl_y), (nx, y));
                                    px = nx;
                                    up = !up;
                                }
                                let stroke = Stroke::new(size).with_caps(Cap::Round);
                                scene.stroke(&stroke, transform, brush, None, &path);
                            }
                        }
                    };

                // Resolve the CSS `text-decoration-thickness` to a device-pixel size.
                //
                // - Percentages are resolved against the font size,
                // - `from-font` uses the metrics from Parley
                // - `auto` uses a thickness of `font-size / 10` (minimum: 1px).
                let decoration_size = |metric_size: f32| match &text_decoration_thickness {
                    GenericTextDecorationLength::LengthPercentage(lp) => {
                        let css_font_size = font_size as f64 / scale;
                        lp.resolve(Length::new(css_font_size as f32)).px() as f64 * scale
                    }
                    GenericTextDecorationLength::FromFont => metric_size as f64,
                    GenericTextDecorationLength::Auto => {
                        let css_font_size = font_size as f64 / scale;
                        (css_font_size / 10.0).max(1.0) * scale
                    }
                } as f32;

                if has_underline {
                    let size = decoration_size(metrics.underline_size);

                    // Apply the CSS `text-underline-offset`, which moves the underline further
                    // away from the text. `auto` keeps the font's suggested position. The value
                    // is resolved against the font size (percentages are relative to 1em) in CSS
                    // pixels, then scaled to device pixels to match the (already scaled) glyph
                    // metrics.
                    let extra_offset = itext_styles
                        .text_underline_offset
                        .non_auto()
                        .map(|lp| {
                            let css_font_size = font_size as f64 / scale;
                            lp.resolve(Length::new(css_font_size as f32)).px() as f64 * scale
                        })
                        .unwrap_or(0.0);

                    // `text-underline-position: under` places the underline below the glyph
                    // box (below descenders) rather than at the font's suggested position near
                    // the alphabetic baseline. We anchor the top of the line at the descent so
                    // it clears descending glyphs like "gqy".
                    //
                    // `draw_decoration_line` computes `y = baseline - offset + size / 2` and `y`
                    // grows downward, so a more negative offset pushes the underline downward,
                    // away from the text.
                    let base_offset = if itext_styles
                        .text_underline_position
                        .contains(TextUnderlinePosition::UNDER)
                    {
                        -metrics.descent
                    } else {
                        metrics.underline_offset
                    };
                    let offset = base_offset - extra_offset as f32;

                    // TODO: intercept line when crossing an descending character like "gqy"
                    // A `double` underline extends downward, away from the text.
                    draw_decoration_line(offset, size, &text_decoration_brush, 1.0);
                }
                if has_overline {
                    // Fonts don't provide a dedicated overline metric, so reuse the underline
                    // thickness. The line sits at the top of the "em box": its lower edge rests
                    // on the ascent so it clears the glyphs, and it extends upward from there
                    // (`draw_decoration_line` centres the stroke on `baseline - offset + size / 2`,
                    // so `offset = ascent + size` puts the bottom edge at `baseline - ascent`).
                    //
                    // Browsers use the OS/2 `usWinAscent` for this edge, which is taller than the
                    // hhea-based ascent Parley reports; using the smaller value would draw the
                    // overline too low (too close to the glyphs). Fall back to Parley's ascent for
                    // fonts without a usable OS/2 table.
                    let size = decoration_size(metrics.underline_size);
                    let ascent = win_ascent(run.font(), run.font_size()).unwrap_or(metrics.ascent);
                    let offset = ascent + size;

                    // A `double` overline extends upward, away from the text.
                    draw_decoration_line(offset, size, &text_decoration_brush, -1.0);
                }
                if has_strikethrough {
                    let size = decoration_size(metrics.strikethrough_size);

                    // Centre the line-through a third of the ascent above the baseline, matching
                    // Chrome (which places it `2/3 * ascent` below the text-top). Parley's
                    // `strikethrough_offset` (the font's `yStrikeoutPosition`) sits lower and would
                    // draw the line too close to the baseline. `draw_decoration_line` centres the
                    // stroke on `baseline - offset + size / 2`, so `offset = ascent / 3 + size / 2`
                    // puts the centre at `baseline - ascent / 3`.
                    let ascent = win_ascent(run.font(), run.font_size()).unwrap_or(metrics.ascent);
                    let offset = ascent / 3.0 + size / 2.0;

                    draw_decoration_line(offset, size, &text_decoration_brush, 1.0);
                }
            }
        }
    }
}

/// Draw selection highlight rectangles for the given byte range in a layout.
/// Uses Parley's Selection type for accurate geometry calculation.
pub(crate) fn draw_text_selection(
    scene: &mut impl PaintScene,
    layout: &Layout<TextBrush>,
    transform: Affine,
    selection_start: usize,
    selection_end: usize,
) {
    let anchor = Cursor::from_byte_index(layout, selection_start, Affinity::Downstream);
    let focus = Cursor::from_byte_index(layout, selection_end, Affinity::Downstream);
    let selection = Selection::new(anchor, focus);

    selection.geometry_with(layout, |rect, _line_idx| {
        let rect = kurbo::Rect::new(rect.x0, rect.y0, rect.x1, rect.y1);
        scene.fill(Fill::NonZero, transform, SELECTION_COLOR, None, &rect);
    });
}
