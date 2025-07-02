use anyrender::PaintScene;
use blitz_dom::node::TextBrush;
use kurbo::{Affine, Point, Stroke};
use parley::{Line, PositionedLayoutItem};
use peniko::Fill;

pub(crate) fn stroke_text<'a>(
    scale: f64,
    scene: &mut impl PaintScene,
    lines: impl Iterator<Item = Line<'a, TextBrush>>,
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

                scene.draw_glyphs(
                    font,
                    font_size,
                    false, // hint
                    run.normalized_coords(),
                    Fill::NonZero,
                    &style.brush.brush,
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

                let mut draw_decoration_line = |offset: f32, size: f32, brush: &TextBrush| {
                    let x = glyph_run.offset() as f64;
                    let w = glyph_run.advance() as f64;
                    let y = (glyph_run.baseline() - offset + size / 2.0) as f64;
                    let line = kurbo::Line::new((x, y), (x + w, y));
                    scene.stroke(
                        &Stroke::new(size as f64),
                        transform,
                        &brush.brush,
                        None,
                        &line,
                    )
                };

                if let Some(underline) = &style.underline {
                    let offset = underline.offset.unwrap_or(metrics.underline_offset);
                    let size = underline.size.unwrap_or(metrics.underline_size);

                    // TODO: intercept line when crossing an descending character like "gqy"
                    draw_decoration_line(offset, size, &underline.brush);
                }
                if let Some(strikethrough) = &style.strikethrough {
                    let offset = strikethrough.offset.unwrap_or(metrics.strikethrough_offset);
                    let size = strikethrough.size.unwrap_or(metrics.strikethrough_size);

                    draw_decoration_line(offset, size, &strikethrough.brush);
                }
            }
        }
    }
}
