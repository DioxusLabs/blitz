use anyrender::{CustomPaint, NormalizedCoord, Paint, PaintScene};
use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, BrushRef, Color, Fill, FontData, ImageBrush, StyleRef};
use rustc_hash::FxHashMap;
use vello::Renderer as VelloRenderer;

use crate::{CustomPaintSource, custom_paint_source::CustomPaintCtx};

pub struct VelloScenePainter<'r> {
    pub renderer: &'r mut VelloRenderer,
    pub custom_paint_sources: &'r mut FxHashMap<u64, Box<dyn CustomPaintSource>>,
    pub inner: vello::Scene,
}

impl VelloScenePainter<'_> {
    fn render_custom_source(&mut self, custom_paint: CustomPaint) -> Option<peniko::ImageBrush> {
        let CustomPaint {
            source_id,
            width,
            height,
            scale,
        } = custom_paint;

        // Render custom paint source
        let source = self.custom_paint_sources.get_mut(&source_id)?;
        let ctx = CustomPaintCtx::new(self.renderer);
        let texture_handle = source.render(ctx, width, height, scale)?;

        // Return dummy image
        Some(ImageBrush::new(texture_handle.0))
    }
}

impl VelloScenePainter<'_> {
    pub fn finish(self) -> vello::Scene {
        self.inner
    }
}

impl PaintScene for VelloScenePainter<'_> {
    fn reset(&mut self) {
        self.inner.reset();
    }

    fn push_layer(
        &mut self,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
    ) {
        self.inner.push_layer(blend, alpha, transform, clip);
    }

    fn pop_layer(&mut self) {
        self.inner.pop_layer();
    }

    fn stroke<'a>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        brush: impl Into<BrushRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        self.inner
            .stroke(style, transform, brush, brush_transform, shape);
    }

    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        paint: impl Into<Paint<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let paint: Paint<'_> = paint.into();

        let dummy_image: peniko::ImageBrush;
        let brush_ref: BrushRef<'_> = match paint {
            Paint::Solid(color) => BrushRef::Solid(color),
            Paint::Gradient(gradient) => BrushRef::Gradient(gradient),
            Paint::Image(image) => BrushRef::Image(image),
            Paint::Custom(custom_paint) => {
                let Ok(custom_paint) = custom_paint.downcast::<CustomPaint>() else {
                    return;
                };
                let Some(image) = self.render_custom_source(*custom_paint) else {
                    return;
                };
                dummy_image = image;
                BrushRef::Image(dummy_image.as_ref())
            }
        };

        self.inner
            .fill(style, transform, brush_ref, brush_transform, shape);
    }

    fn draw_glyphs<'a, 's: 'a>(
        &'a mut self,
        font: &'a FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [NormalizedCoord],
        style: impl Into<StyleRef<'a>>,
        brush: impl Into<BrushRef<'a>>,
        brush_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = anyrender::Glyph>,
    ) {
        self.inner
            .draw_glyphs(font)
            .font_size(font_size)
            .hint(hint)
            .normalized_coords(normalized_coords)
            .brush(brush)
            .brush_alpha(brush_alpha)
            .transform(transform)
            .glyph_transform(glyph_transform)
            .draw(
                style,
                glyphs.map(|g: anyrender::Glyph| vello::Glyph {
                    id: g.id,
                    x: g.x,
                    y: g.y,
                }),
            );
    }

    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        brush: Color,
        radius: f64,
        std_dev: f64,
    ) {
        self.inner
            .draw_blurred_rounded_rect(transform, rect, brush, radius, std_dev);
    }
}
