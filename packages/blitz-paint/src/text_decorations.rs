use anyrender::PaintScene;
use anyrender::filters::{Filter, FilterEffect};
use blitz_dom::CopyStyleGetters;
use blitz_dom::{BaseDocument, NodeId, node::TextBrush, util::ToColorColor};
use kurbo::{Affine, BezPath, Cap, Circle, Rect, Stroke, Vec2};
use parley::Line;
use peniko::{Fill, Mix};
use std::collections::HashMap;
use std::sync::Arc;
use style::properties::generated::longhands::text_decoration_style::computed_value::T as TextDecorationStyle;
use style::values::computed::{
    Length, LengthPercentage, TextDecorationLength, TextDecorationLine, TextUnderlinePosition,
};
use style::values::generics::text::{GenericTextDecorationInset, GenericTextDecorationLength};

use crate::color::{Color, ToColorColor as _};
use crate::text::{RunRecord, TextPaintOptions, draw_glyph_run, glyph_run_rect, glyph_runs};

/// Per-font-face cache of the OS/2 `usWinAscent / unitsPerEm` ratio, keyed by the font
/// blob's unique id and face index. `None` is cached for fonts without a usable OS/2 or
/// head table so they aren't re-parsed either.
pub(crate) type WinAscentCache = HashMap<(u64, u32), Option<f32>>;

/// The font's OS/2 `usWinAscent`, in the same (device) pixels as `font_size`.
///
/// This is the ascent browsers use as the top of the "em box" when positioning overlines. It
/// is typically taller than the hhea ascent Parley exposes via [`parley::layout::run::RunMetrics`].
/// Returns `None` when the font has no OS/2 table.
fn win_ascent(cache: &mut WinAscentCache, font: &parley::FontData, font_size: f32) -> Option<f32> {
    let ratio = *cache
        .entry((font.data.id(), font.index))
        .or_insert_with(|| win_ascent_ratio(font));
    Some(ratio? * font_size)
}

/// The unitless `usWinAscent / unitsPerEm` ratio for a font face, or `None` when the font
/// has no usable head/OS/2 table.
fn win_ascent_ratio(font: &parley::FontData) -> Option<f32> {
    use skrifa::raw::{FontRef, TableProvider as _};
    let font_ref = FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let units_per_em = font_ref.head().ok()?.units_per_em();
    let win_ascent = font_ref.os2().ok()?.us_win_ascent();
    Some(win_ascent as f32 / units_per_em as f32)
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

/// A text decoration resolved from a single "decorating box" (an element that
/// sets `text-decoration-line`), ready to be drawn over a glyph run. Because the
/// `text-decoration-*` properties do not inherit, decorations are propagated from
/// ancestors rather than inherited, so a run may be covered by several of these.
///
/// Only the run-independent (style-derived) values are stored here so an instance
/// can be cached on the ancestor stack and reused across every run within the same
/// decorating box. The `text-decoration-inset` and `text-underline-offset` values
/// depend on the run's advance/font-size and are resolved lazily when drawing.
#[derive(Clone)]
pub(crate) struct ResolvedDecoration {
    line: TextDecorationLine,
    style: TextDecorationStyle,
    color: Color,
    thickness: TextDecorationLength,
    /// `text-underline-offset`; `None` for `auto`. Resolved per run.
    underline_offset: Option<LengthPercentage>,
    /// Whether `text-underline-position: under` is in effect.
    underline_under: bool,
    /// Whether `text-underline-position: from-font` is in effect.
    underline_from_font: bool,
    /// `text-decoration-inset`. Resolved per run.
    inset: GenericTextDecorationInset<LengthPercentage>,
}

/// An element on the current ancestor path (inline root -> run node), caching the
/// values resolved from its computed style so descending into the same node across
/// consecutive runs doesn't re-resolve them.
pub(crate) struct DecorationStackEntry {
    pub(crate) node_id: NodeId,
    /// This node's (inherited) text fill colour, used for the glyphs of runs whose
    /// innermost node is this one.
    pub(crate) fill_color: Color,
    /// The decoration this node introduces as a decorating box, if any.
    pub(crate) decoration: Option<ResolvedDecoration>,
    /// This node's (inherited) `text-shadow` list, or `None` for `text-shadow: none`.
    /// Shared (via `Arc`) with the per-run records and decorating boxes so that
    /// consecutive runs with the same shadow can be batched by pointer equality.
    pub(crate) text_shadows: Option<Arc<[ResolvedTextShadow]>>,
}

/// One `text-shadow` layer with all lengths resolved to device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedTextShadow {
    offset: Vec2,
    /// Gaussian standard deviation (half the CSS blur radius); `0.0` for a sharp shadow.
    std_dev: f64,
    color: Color,
}

impl ResolvedTextShadow {
    fn is_blurred(&self) -> bool {
        self.std_dev > 0.0
    }
}

/// Resolve the cached style values for a single node into a [`DecorationStackEntry`].
pub(crate) fn resolve_decoration_entry(
    doc: &BaseDocument,
    node_id: NodeId,
    parent: Option<&DecorationStackEntry>,
    scale: f64,
) -> DecorationStackEntry {
    let Some(styles) = doc.get_node(node_id).and_then(|node| node.primary_styles()) else {
        return DecorationStackEntry {
            node_id,
            fill_color: Color::BLACK,
            decoration: None,
            text_shadows: None,
        };
    };

    let itext = styles.get_inherited_text();
    let text = styles.get_text();
    let text_color = itext.color.as_color_color();
    let fill_color = itext
        ._webkit_text_fill_color
        .resolve_to_absolute(&itext.color)
        .as_srgb_color();

    // `text-shadow` inherits, so the run's innermost node carries the effective list.
    // Lengths are in CSS pixels; glyph runs are laid out in device pixels, so scale here.
    let text_shadows = (!itext.text_shadow.0.is_empty()).then(|| {
        itext
            .text_shadow
            .0
            .iter()
            .map(|shadow| ResolvedTextShadow {
                offset: Vec2::new(
                    shadow.horizontal.px() as f64 * scale,
                    shadow.vertical.px() as f64 * scale,
                ),
                // CSS specifies the blur as a radius approximating a Gaussian with a
                // standard deviation of half that radius.
                std_dev: shadow.blur.px() as f64 * scale / 2.0,
                color: shadow
                    .color
                    .resolve_to_absolute(&itext.color)
                    .as_srgb_color(),
            })
            .collect::<Arc<[_]>>()
    });
    // `text-shadow` is usually inherited unchanged, so share the parent's list: the shadow
    // painter batches runs by `Arc` identity, and this keeps e.g. `<b>`/`<i>` children of
    // a shadowed span in the same batch as their parent's text.
    let text_shadows = match (text_shadows, parent.and_then(|p| p.text_shadows.as_ref())) {
        (Some(own), Some(inherited)) if *own == **inherited => Some(inherited.clone()),
        (own, _) => own,
    };

    let drawn_lines = TextDecorationLine::UNDERLINE
        | TextDecorationLine::OVERLINE
        | TextDecorationLine::LINE_THROUGH;
    let line = text.text_decoration_line;
    // Decorations propagate through the box tree, and a `display: contents` element
    // generates no box, so its decorations have no effect on descendants.
    let is_contents = styles.clone_display().is_contents();
    let decoration = (!is_contents && line.intersects(drawn_lines)).then(|| {
        // `text-decoration-color: currentColor` (the initial value) resolves against
        // the decorating box's own colour, not the descendant run's.
        let color = text
            .text_decoration_color
            .as_absolute()
            .map(ToColorColor::as_color_color)
            .unwrap_or(text_color);

        ResolvedDecoration {
            line,
            style: text.text_decoration_style,
            color,
            thickness: text.text_decoration_thickness.clone(),
            underline_offset: itext.text_underline_offset.non_auto(),
            underline_under: itext
                .text_underline_position
                .contains(TextUnderlinePosition::UNDER),
            underline_from_font: itext
                .text_underline_position
                .contains(TextUnderlinePosition::FROM_FONT),
            inset: text.text_decoration_inset.clone(),
        }
    });

    DecorationStackEntry {
        node_id,
        fill_color,
        decoration,
        text_shadows,
    }
}

/// The run-dependent geometry used to position and size a decoration, captured from a
/// single glyph run. Firefox draws a decoration once for the whole decorating box using
/// the box's own font, so we prefer the values from a run whose innermost node *is* the
/// decorating box (its own text), falling back to the first run it covers.
#[derive(Clone)]
pub(crate) struct DecorationRunGeometry {
    /// The baseline the decoration is positioned from (the run's own, `vertical-align`
    /// shifted, baseline).
    pub(crate) baseline: f32,
    pub(crate) ascent: f32,
    pub(crate) descent: f32,
    pub(crate) underline_offset: f32,
    pub(crate) underline_size: f32,
    pub(crate) strikethrough_size: f32,
    pub(crate) font: parley::FontData,
    pub(crate) font_size: f32,
    pub(crate) css_font_size: f64,
}

/// A decoration accumulated across the runs of a single line for one decorating box.
///
/// The `text-decoration-*` properties describe the whole box, so we gather the horizontal
/// extent it covers on the line (`min_x`/`max_x`) and the font geometry to draw with, then
/// paint a single line spanning the box rather than one segment per run.
pub(crate) struct LineDecoration {
    pub(crate) node_id: NodeId,
    pub(crate) deco: ResolvedDecoration,
    pub(crate) min_x: f64,
    pub(crate) max_x: f64,
    /// Geometry from a run whose innermost node is the decorating box itself.
    pub(crate) own: Option<DecorationRunGeometry>,
    /// Geometry from the first run the box covers (fallback when it has no own text on
    /// this line, e.g. it only wraps differently-sized descendants).
    pub(crate) first: Option<DecorationRunGeometry>,
}

/// Resolve the CSS `text-decoration-thickness` to a device-pixel size.
///
/// - Percentages are resolved against the font size,
/// - `from-font` uses the metrics from Parley,
/// - `auto` uses a thickness of `font-size / 10` (minimum: 1px).
///
/// The result is floored to a whole number of device pixels (minimum 1), matching
/// browsers, which snap decoration thickness to whole pixels.
fn decoration_size(
    thickness: &TextDecorationLength,
    metric_size: f32,
    css_font_size: f64,
    scale: f64,
) -> f32 {
    (match thickness {
        GenericTextDecorationLength::LengthPercentage(lp) => {
            lp.resolve(Length::new(css_font_size as f32)).px() as f64 * scale
        }
        GenericTextDecorationLength::FromFont => metric_size as f64,
        GenericTextDecorationLength::Auto => (css_font_size / 10.0).max(1.0) * scale,
    })
    .floor()
    .max(1.0) as f32
}

/// Draws a single decoration line of the given `text-decoration-style` spanning
/// `[x0, x0 + width]`. `baseline` is the line's baseline and `offset` positions the line
/// relative to it (`y = baseline - offset + size / 2`, growing downward). Geometry is built
/// explicitly (rather than relying on backend dash support) so it renders consistently
/// across renderers.
///
/// `double_dir` points away from the text (`+1` = down, `-1` = up) and is used to place the
/// second line of a `double` decoration.
#[allow(clippy::too_many_arguments)]
fn draw_decoration_line(
    scene: &mut impl PaintScene,
    transform: Affine,
    scale: f64,
    x0: f64,
    width: f64,
    baseline: f32,
    offset: f32,
    size: f32,
    brush: &anyrender::Paint,
    double_dir: f64,
    deco_style: TextDecorationStyle,
    inset_start: f64,
    inset_end: f64,
) {
    // Inset the line from the box's start/end edges (assumes LTR: start is the left edge).
    // Negative insets extend the line past the text.
    let x = x0 + inset_start;
    let w = width - inset_start - inset_end;
    if w <= 0.0 {
        return;
    }
    let y = (baseline - offset + size / 2.0) as f64;
    let size = size as f64;
    let butt_stroke = Stroke::new(size).with_caps(Cap::Butt);

    match deco_style {
        TextDecorationStyle::None => {
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
            let (dash_ratio, gap_ratio) = if size >= 3.0 { (2.0, 1.0) } else { (3.0, 2.0) };
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
}

/// Paint the decorations accumulated for one line, one line per decorating box.
///
/// When `shadow` is set, the decorations are drawn as that text shadow: offset, in the
/// shadow colour and clipped to the given rect (the extent of the shadowed runs), since a
/// decoration takes the `text-shadow` of the text it is drawn under, as in Blink and Gecko.
#[allow(clippy::too_many_arguments)]
pub(crate) fn flush_line_decorations(
    scene: &mut impl PaintScene,
    transform: Affine,
    options: TextPaintOptions,
    deco_boxes: &[LineDecoration],
    win_ascent_ratios: &mut WinAscentCache,
    inline_root_id: NodeId,
    line_baseline: f32,
    shadow: Option<(&ResolvedTextShadow, Rect)>,
) {
    let TextPaintOptions {
        scale,
        brush_override,
    } = options;
    let (transform, brush_override) = match shadow {
        Some((shadow, _)) => (
            transform * Affine::translate(shadow.offset),
            Some(shadow.color),
        ),
        None => (transform, brush_override),
    };
    // Draw innermost boxes first so ancestors' decorations paint on top, matching the
    // per-run drawing order this replaced (`stack.iter().rev()`).
    for acc in deco_boxes.iter().rev() {
        let shadow_clip = shadow.map(|(_, clip)| clip);
        if let Some(clip) = shadow_clip
            && (acc.max_x <= clip.x0 || acc.min_x >= clip.x1)
        {
            continue;
        }

        let deco = &acc.deco;
        // Prefer the decorating box's own font; fall back to the first run it covers.
        let geom = match (&acc.own, &acc.first) {
            (Some(own), _) => own.clone(),
            (None, Some(first)) => {
                let mut geom = first.clone();
                // A descendant run's baseline may be shifted by `vertical-align`, but the
                // decoration is positioned from the decorating box's own baseline. For the
                // inline root that is the line's baseline.
                if acc.node_id == inline_root_id {
                    geom.baseline = line_baseline;
                }
                geom
            }
            (None, None) => continue,
        };
        let geom = &geom;
        let width = acc.max_x - acc.min_x;
        if width <= 0.0 {
            continue;
        }
        let clip = shadow_clip.filter(|clip| acc.min_x < clip.x0 || acc.max_x > clip.x1);
        if let Some(clip) = &clip {
            scene.push_clip_layer(Fill::NonZero, transform, clip);
        }
        let brush = anyrender::Paint::from(brush_override.unwrap_or(deco.color));

        // `text-decoration-inset` shortens (or, when negative, extends) the line from the
        // inline-start/end edges. Percentages resolve against the decoration line length
        // (the box's total advance on this line); `auto` is no inset.
        let (inset_start, inset_end) = match &deco.inset {
            GenericTextDecorationInset::LengthPercentage { start, end } => {
                // The extent is in device pixels; convert to CSS pixels so the resolved
                // result can be scaled back.
                let line_length = Length::new((width / scale) as f32);
                (
                    start.resolve(line_length).px() as f64 * scale,
                    end.resolve(line_length).px() as f64 * scale,
                )
            }
            GenericTextDecorationInset::Auto => (0.0, 0.0),
        };

        if deco.line.contains(TextDecorationLine::UNDERLINE) {
            let size = decoration_size(
                &deco.thickness,
                geom.underline_size,
                geom.css_font_size,
                scale,
            );

            // `text-underline-offset` moves the underline away from the text. `auto` keeps
            // the font's suggested position; otherwise it resolves against the font size
            // (percentages relative to 1em).
            let extra_underline_offset = deco
                .underline_offset
                .as_ref()
                .map(|lp| lp.resolve(Length::new(geom.css_font_size as f32)).px() as f64 * scale)
                .unwrap_or(0.0);

            // `text-underline-position: under` places the underline below the glyph box
            // (below descenders) rather than at the font's suggested position near the
            // alphabetic baseline. We anchor the top of the line at the descent so it clears
            // descending glyphs like "gqy".
            // An explicit offset under `auto` is taken from the alphabetic
            // baseline, as CSS and Chrome have it, not from the auto gap.
            let base_offset = if deco.underline_under {
                -geom.descent
            } else if deco.underline_offset.is_some() && !deco.underline_from_font {
                0.0
            } else {
                geom.underline_offset
            };
            let offset = base_offset - extra_underline_offset as f32;

            // A `double` underline extends downward, away from the text.
            draw_decoration_line(
                scene,
                transform,
                scale,
                acc.min_x,
                width,
                geom.baseline,
                offset,
                size,
                &brush,
                1.0,
                deco.style,
                inset_start,
                inset_end,
            );
        }
        if deco.line.contains(TextDecorationLine::OVERLINE) {
            // Fonts don't provide a dedicated overline metric, so reuse the underline
            // thickness. The line sits at the top of the "em box": its lower edge rests on
            // the ascent so it clears the glyphs, and it extends upward from there.
            //
            // Browsers use the OS/2 `usWinAscent` for this edge, which is taller than the
            // hhea-based ascent Parley reports; using the smaller value would draw the
            // overline too low. Fall back to Parley's ascent for fonts without a usable
            // OS/2 table.
            let size = decoration_size(
                &deco.thickness,
                geom.underline_size,
                geom.css_font_size,
                scale,
            );
            let ascent =
                win_ascent(win_ascent_ratios, &geom.font, geom.font_size).unwrap_or(geom.ascent);
            let offset = ascent + size;

            // A `double` overline extends upward, away from the text.
            draw_decoration_line(
                scene,
                transform,
                scale,
                acc.min_x,
                width,
                geom.baseline,
                offset,
                size,
                &brush,
                -1.0,
                deco.style,
                inset_start,
                inset_end,
            );
        }
        if deco.line.contains(TextDecorationLine::LINE_THROUGH) {
            let size = decoration_size(
                &deco.thickness,
                geom.strikethrough_size,
                geom.css_font_size,
                scale,
            );

            // Centre the line-through a third of the ascent above the baseline, matching
            // Chrome (which places it `2/3 * ascent` below the text-top). Parley's
            // `strikethrough_offset` (the font's `yStrikeoutPosition`) sits lower and would
            // draw the line too close to the baseline.
            let ascent =
                win_ascent(win_ascent_ratios, &geom.font, geom.font_size).unwrap_or(geom.ascent);
            let offset = ascent / 3.0 + size / 2.0;

            draw_decoration_line(
                scene,
                transform,
                scale,
                acc.min_x,
                width,
                geom.baseline,
                offset,
                size,
                &brush,
                1.0,
                deco.style,
                inset_start,
                inset_end,
            );
        }
        if clip.is_some() {
            scene.pop_layer();
        }
    }
}

/// Paint the `text-shadow`s of one line.
///
/// Like Blink and Gecko, a shadow is painted by drawing the text again, offset and in the
/// shadow colour, beneath the real text; the shadows in a list are drawn last-to-first so
/// the first one ends up on top. An unblurred shadow is just an extra glyph draw. A
/// blurred one needs a blur filter layer, which is the expensive part, so consecutive runs
/// that share the same `text-shadow` list (by `Arc` identity: a paragraph's runs normally
/// all inherit one list) are batched into a single layer per shadow.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_line_text_shadows(
    scene: &mut impl PaintScene,
    line: &Line<'_, TextBrush>,
    run_records: &[RunRecord],
    deco_boxes: &[LineDecoration],
    win_ascent_ratios: &mut WinAscentCache,
    transform: Affine,
    options: TextPaintOptions,
    inline_root_id: NodeId,
) {
    let line_metrics = line.metrics();
    let line_baseline = line_metrics.baseline;
    let line_height = (line_metrics.block_max_coord - line_metrics.block_min_coord) as f64;

    let mut start = 0;
    while start < run_records.len() {
        let Some(shadow_list) = &run_records[start].shadows else {
            start += 1;
            continue;
        };

        // The batch: this run plus following runs with the same shadow list.
        let end = start
            + run_records[start..]
                .iter()
                .take_while(|record| {
                    record
                        .shadows
                        .as_ref()
                        .is_some_and(|list| Arc::ptr_eq(list, shadow_list))
                })
                .count();
        let batch = || glyph_runs(line).skip(start).take(end - start);

        let glyph_bounds = batch()
            .map(|run| glyph_run_rect(&run))
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::ZERO);
        // Decorations drawn under these runs are shadowed with them, clipped to the runs'
        // horizontal extent. The vertical extent is generous to cover any decoration style.
        let has_decorations = deco_boxes
            .iter()
            .any(|acc| acc.max_x > glyph_bounds.x0 && acc.min_x < glyph_bounds.x1);
        let deco_clip = Rect::new(
            glyph_bounds.x0,
            line_metrics.block_min_coord as f64 - line_height,
            glyph_bounds.x1,
            line_metrics.block_max_coord as f64 + line_height,
        );
        let batch_bounds = if has_decorations {
            glyph_bounds.union(deco_clip)
        } else {
            glyph_bounds
        };

        for shadow in shadow_list.iter().rev() {
            if shadow.color.components[3] <= 0.0 {
                continue;
            }
            if shadow.is_blurred() {
                let filter = Arc::new(Filter::single(FilterEffect::blur(shadow.std_dev as f32)));
                // Layer clip: the shadowed content's bounds, offset, plus room for the blur.
                let margin = 3.0 * shadow.std_dev;
                let clip = (batch_bounds + shadow.offset).inflate(margin, margin);
                scene.push_layer(
                    Fill::NonZero,
                    Mix::Normal,
                    1.0,
                    transform,
                    &clip,
                    Some(filter),
                    None,
                );
            }

            let shadow_transform = transform * Affine::translate(shadow.offset);
            for run in batch() {
                draw_glyph_run(scene, &run, shadow.color, shadow_transform);
            }
            if has_decorations {
                flush_line_decorations(
                    scene,
                    transform,
                    options,
                    deco_boxes,
                    win_ascent_ratios,
                    inline_root_id,
                    line_baseline,
                    Some((shadow, deco_clip)),
                );
            }

            if shadow.is_blurred() {
                scene.pop_layer();
            }
        }

        start = end;
    }
}
