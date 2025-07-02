use anyrender::PaintScene;
use blitz_dom::{BaseDocument, node::TextBrush, util::ToColorColor};
use kurbo::{Affine, Point, Stroke};
use parley::{Line, PositionedLayoutItem};
use peniko::Fill;
use style::values::computed::TextDecorationLine;

pub(crate) fn stroke_text<'a>(
    scale: f64,
    scene: &mut impl PaintScene,
    lines: impl Iterator<Item = Line<'a, TextBrush>>,
    doc: &BaseDocument,
    pos: Point,
) {
    let transform = Affine::translate((pos.x * scale, pos.y * scale));
    for line in lines {
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                let mut x = glyph_run.offset();
                let y = glyph_run.baseline();

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
                let text_decoration_brush = peniko::Brush::from(text_decoration_color);
                let text_decoration_line = text_styles.text_decoration_line;
                let has_underline = text_decoration_line.contains(TextDecorationLine::UNDERLINE);
                let has_strikethrough =
                    text_decoration_line.contains(TextDecorationLine::LINE_THROUGH);

                scene.draw_glyphs(
                    font,
                    font_size,
                    false, // hint
                    run.normalized_coords(),
                    Fill::NonZero,
                    &peniko::Brush::from(text_color),
                    1.0, // alpha
                    transform,
                    glyph_xform,
                    glyph_run.glyphs().map(|glyph| {
                        let gx = x + glyph.x;
                        let gy = y - glyph.y;
                        x += glyph.advance;

                        anyrender::Glyph {
                            id: glyph.id as _,
                            x: gx,
                            y: gy,
                        }
                    }),
                );

                let mut draw_decoration_line = |offset: f32, size: f32, brush: &peniko::Brush| {
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
                    draw_decoration_line(offset, size, &text_decoration_brush);
                }
                if has_strikethrough {
                    let offset = metrics.strikethrough_offset;
                    let size = metrics.strikethrough_size;

                    draw_decoration_line(offset, size, &text_decoration_brush);
                }
            }
        }
    }
}
