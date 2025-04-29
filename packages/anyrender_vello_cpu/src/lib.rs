use std::sync::Arc;

use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, BrushRef, Color, Fill, Font, StyleRef};

use anyrender::{DrawGlyphs, NormalizedCoord, Scene};
use vello_common::paint::PaintType;
use vello_cpu::Pixmap;

type VelloCpuGlyphBuilder<'a> = vello_common::glyph::GlyphRunBuilder<'a, vello_cpu::RenderContext>;

pub struct VelloCpuAnyrenderScene(pub vello_cpu::RenderContext);

pub struct VelloCpuAnyrenderGlyphBuilder<'a>(pub VelloCpuGlyphBuilder<'a>);

fn brush_ref_to_paint_type<'a>(brush_ref: BrushRef<'a>) -> PaintType {
    match brush_ref {
        BrushRef::Solid(alpha_color) => PaintType::Solid(alpha_color),
        BrushRef::Gradient(gradient) => PaintType::Gradient(gradient.clone()),
        BrushRef::Image(image) => PaintType::Image(vello_common::paint::Image {
            pixmap: Arc::new(Pixmap {
                width: image.width as u16,
                height: image.height as u16,
                buf: Vec::from(image.data.as_ref()),
            }),
            x_extend: image.x_extend,
            y_extend: image.y_extend,
            quality: image.quality,
        }),
    }
}

const DEFAULT_TOLERANCE: f64 = 0.1;

impl Scene for VelloCpuAnyrenderScene {
    type Output = vello_cpu::Pixmap;

    type GlyphBuilder<'a>
        = VelloCpuAnyrenderGlyphBuilder<'a>
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
        self.0.set_transform(transform);
        self.0.push_layer(
            Some(&clip.into_path(DEFAULT_TOLERANCE)),
            Some(blend.into()),
            Some((alpha * 255.0) as u8),
            None,
        );
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
        self.0.set_transform(transform);
        self.0.set_stroke(style.clone());
        self.0.set_paint(brush_ref_to_paint_type(brush.into()));
        self.0
            .set_paint_transform(brush_transform.unwrap_or(Affine::IDENTITY));
        self.0.stroke_path(&shape.into_path(DEFAULT_TOLERANCE));
    }

    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        brush: impl Into<BrushRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        self.0.set_transform(transform);
        self.0.set_fill_rule(style);
        self.0.set_paint(brush_ref_to_paint_type(brush.into()));
        self.0
            .set_paint_transform(brush_transform.unwrap_or(Affine::IDENTITY));
        self.0.fill_path(&shape.into_path(DEFAULT_TOLERANCE));
    }

    fn draw_glyphs(&mut self, font: &Font) -> Self::GlyphBuilder<'_> {
        VelloCpuAnyrenderGlyphBuilder(self.0.glyph_run(font))
    }

    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        color: Color,
        radius: f64,
        std_dev: f64,
    ) {
        self.0.set_transform(transform);
        self.0.set_paint(PaintType::Solid(color));
        self.0
            .fill_blurred_rounded_rect(&rect, radius as f32, std_dev as f32);
    }

    fn finish(self) -> Self::Output {
        let mut pixmap = Pixmap::new(self.0.width(), self.0.height());
        self.0.render_to_pixmap(&mut pixmap);
        pixmap
    }
}

impl<'a> DrawGlyphs<'a> for VelloCpuAnyrenderGlyphBuilder<'a> {
    fn font_size(self, size: f32) -> Self {
        Self(self.0.font_size(size))
    }

    fn hint(self, hint: bool) -> Self {
        Self(self.0.hint(hint))
    }

    fn normalized_coords(self, coords: &'a [NormalizedCoord]) -> Self {
        Self(self.0.normalized_coords(coords))
    }

    fn brush(self, _brush: impl Into<BrushRef<'a>>) -> Self {
        self // TODO
    }

    fn brush_alpha(self, _alpha: f32) -> Self {
        self // TODO
    }

    fn transform(self, _transform: Affine) -> Self {
        self // TODO
    }

    fn glyph_transform(self, transform: Option<Affine>) -> Self {
        Self(self.0.glyph_transform(transform.unwrap_or_default()))
    }

    fn draw(self, style: impl Into<StyleRef<'a>>, glyphs: impl Iterator<Item = anyrender::Glyph>) {
        fn into_vello_cpu_glyph(g: anyrender::Glyph) -> vello_common::glyph::Glyph {
            vello_common::glyph::Glyph {
                id: g.id,
                x: g.x,
                y: g.y,
            }
        }

        let style: StyleRef<'a> = style.into();
        match style {
            StyleRef::Fill(_fill) => {
                // TODO: set fill style
                self.0.fill_glyphs(glyphs.map(into_vello_cpu_glyph))
            }
            StyleRef::Stroke(_stroke) => {
                // TODO: set stroke style
                self.0.stroke_glyphs(glyphs.map(into_vello_cpu_glyph))
            }
        }
    }
}
