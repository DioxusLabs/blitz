use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, BrushRef, Color, Fill, Font, StyleRef};

use anyrender::{DrawGlyphs, NormalizedCoord, Scene};

pub struct VelloAnyrenderScene(pub vello::Scene);

pub struct VelloAnyrenderGlyphBuilder<'a>(pub vello::DrawGlyphs<'a>);

impl Scene for VelloAnyrenderScene {
    type Output = vello::Scene;

    type GlyphBuilder<'a>
        = VelloAnyrenderGlyphBuilder<'a>
    where
        Self: 'a;

    fn reset(&mut self) {
        self.0.reset();
    }

    fn push_layer(
        &mut self,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
    ) {
        self.0.push_layer(blend, alpha, transform, clip);
    }

    fn pop_layer(&mut self) {
        self.0.pop_layer();
    }

    fn stroke<'a>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        brush: impl Into<BrushRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        self.0
            .stroke(style, transform, brush, brush_transform, shape);
    }

    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        brush: impl Into<BrushRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        self.0.fill(style, transform, brush, brush_transform, shape);
    }

    fn draw_glyphs(&mut self, font: &Font) -> Self::GlyphBuilder<'_> {
        VelloAnyrenderGlyphBuilder(self.0.draw_glyphs(font))
    }

    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        brush: Color,
        radius: f64,
        std_dev: f64,
    ) {
        self.0
            .draw_blurred_rounded_rect(transform, rect, brush, radius, std_dev);
    }

    fn finish(self) -> Self::Output {
        self.0
    }
}

impl<'a> DrawGlyphs<'a> for VelloAnyrenderGlyphBuilder<'a> {
    fn font_size(self, size: f32) -> Self {
        Self(self.0.font_size(size))
    }

    fn hint(self, hint: bool) -> Self {
        Self(self.0.hint(hint))
    }

    fn normalized_coords(self, coords: &[NormalizedCoord]) -> Self {
        Self(self.0.normalized_coords(coords))
    }

    fn brush(self, brush: impl Into<BrushRef<'a>>) -> Self {
        Self(self.0.brush(brush))
    }

    fn brush_alpha(self, alpha: f32) -> Self {
        Self(self.0.brush_alpha(alpha))
    }

    fn transform(self, transform: Affine) -> Self {
        Self(self.0.transform(transform))
    }

    fn glyph_transform(self, transform: Option<Affine>) -> Self {
        Self(self.0.glyph_transform(transform))
    }

    fn draw(self, style: impl Into<StyleRef<'a>>, glyphs: impl Iterator<Item = anyrender::Glyph>) {
        self.0.draw(
            style,
            glyphs.map(|g: anyrender::Glyph| vello::Glyph {
                id: g.id,
                x: g.x,
                y: g.y,
            }),
        );
    }
}
