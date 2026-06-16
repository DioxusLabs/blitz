use std::sync::Arc;

use anyrender::PaintScene;
use blitz_dom::{
    BaseDocument,
    node::TextBrush,
    util::{Color, ToColorColor},
};
use kurbo::{Affine, Rect, Stroke};
use parley::{
    Affinity, Cursor, GlyphRun, Layout, Line, PositionedLayoutItem, RunMetrics, Selection,
};
use peniko::{Fill, Mix};
use style::values::computed::TextDecorationLine;

use crate::{FONT_EMBOLDEN_ENABLED, SELECTION_COLOR};

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
                let metrics = glyph_run.run().metrics();
                let style = glyph_run.style();

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
                let has_underline = text_decoration_line.contains(TextDecorationLine::UNDERLINE);
                let has_strikethrough =
                    text_decoration_line.contains(TextDecorationLine::LINE_THROUGH);
                let text_shadow = styles.clone_text_shadow();
                let text_shadow_filter = crate::filters::convert_text_shadows(
                    &text_shadow,
                    &itext_styles.color,
                    scale as f32,
                )
                .map(Arc::new);

                if let Some(filter) = text_shadow_filter {
                    let mut clip = glyph_run_rect(&glyph_run, metrics);
                    let expansion = filter.expansion_rect();
                    clip.x0 += expansion.x0;
                    clip.y0 += expansion.y0;
                    clip.x1 += expansion.x1;
                    clip.y1 += expansion.y1;

                    scene.push_layer(Mix::Normal, 1.0, transform, &clip, Some(filter), None);
                    draw_glyph_run(
                        scene,
                        &glyph_run,
                        text_color,
                        &text_decoration_brush,
                        has_underline,
                        has_strikethrough,
                        transform,
                        scale,
                    );
                    scene.pop_layer();
                } else {
                    draw_glyph_run(
                        scene,
                        &glyph_run,
                        text_color,
                        &text_decoration_brush,
                        has_underline,
                        has_strikethrough,
                        transform,
                        scale,
                    );
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_glyph_run(
    scene: &mut impl PaintScene,
    glyph_run: &GlyphRun<'_, TextBrush>,
    text_color: Color,
    text_decoration_brush: &anyrender::Paint,
    has_underline: bool,
    has_strikethrough: bool,
    transform: Affine,
    scale: f64,
) {
    let run = glyph_run.run();
    let font = run.font();
    let font_size = run.font_size();
    let metrics = run.metrics();
    let synthesis = run.synthesis();
    let glyph_xform = synthesis
        .skew()
        .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));

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

    let mut draw_decoration_line = |offset: f32, size: f32, brush: &anyrender::Paint| {
        let x = glyph_run.offset() as f64;
        let w = glyph_run.advance() as f64;
        let y = (glyph_run.baseline() - offset + size / 2.0) as f64;
        let line = kurbo::Line::new((x, y), (x + w, y));
        scene.stroke(&Stroke::new(size as f64), transform, brush, None, &line)
    };

    if has_underline {
        let offset = metrics.underline_offset;
        let size = metrics.underline_size;

        // TODO: intercept line when crossing an descending character like "gqy"
        draw_decoration_line(offset, size, text_decoration_brush);
    }
    if has_strikethrough {
        let offset = metrics.strikethrough_offset;
        let size = metrics.strikethrough_size;

        draw_decoration_line(offset, size, text_decoration_brush);
    }
}

fn glyph_run_rect(glyph_run: &GlyphRun<'_, TextBrush>, metrics: &RunMetrics) -> Rect {
    let x0 = glyph_run.offset() as f64;
    let x1 = (glyph_run.offset() + glyph_run.advance()) as f64;
    let y0 = (glyph_run.baseline() - metrics.ascent - metrics.leading) as f64;
    let y1 = (glyph_run.baseline() + metrics.descent + metrics.leading) as f64;

    Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
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
