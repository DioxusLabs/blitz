use anyrender::{NormalizedCoord, Paint, PaintScene};
use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, BrushRef, Color, Fill, Font, StyleRef, color::PremulRgba8};
use std::sync::Arc;
use vello_cpu::{self, PaintType, Pixmap, RenderMode};

const DEFAULT_TOLERANCE: f64 = 0.1;

fn brush_ref_to_paint_type<'a>(brush_ref: BrushRef<'a>) -> PaintType {
    match brush_ref {
        BrushRef::Solid(alpha_color) => PaintType::Solid(alpha_color),
        BrushRef::Gradient(gradient) => PaintType::Gradient(gradient.clone()),
        BrushRef::Image(image) => PaintType::Image(vello_cpu::Image {
            pixmap: convert_image(image),
            x_extend: image.x_extend,
            y_extend: image.y_extend,
            quality: image.quality,
        }),
    }
}

fn anyrender_paint_to_vello_cpu_paint<'a>(paint: Paint<'a>) -> PaintType {
    match paint {
        Paint::Solid(alpha_color) => PaintType::Solid(alpha_color),
        Paint::Gradient(gradient) => PaintType::Gradient(gradient.clone()),
        Paint::Image(image) => PaintType::Image(vello_cpu::Image {
            pixmap: convert_image(image),
            x_extend: image.x_extend,
            y_extend: image.y_extend,
            quality: image.quality,
        }),
        // TODO: custom paint
        Paint::Custom(_) => PaintType::Solid(peniko::color::palette::css::TRANSPARENT),
    }
}

#[allow(unused)]
fn convert_image_cached(image: &peniko::Image) -> Arc<Pixmap> {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};
    static CACHE: LazyLock<Mutex<HashMap<u64, Arc<Pixmap>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    let mut map = CACHE.lock().unwrap();
    let id = image.data.id();
    let pixmap = map.entry(id).or_insert_with(|| convert_image(image));

    Arc::clone(pixmap)
}

fn convert_image(image: &peniko::Image) -> Arc<Pixmap> {
    Arc::new(Pixmap::from_parts(
        premultiply(image),
        image.width as u16,
        image.height as u16,
    ))
}

fn premultiply(image: &peniko::Image) -> Vec<PremulRgba8> {
    image
        .data
        .as_ref()
        .chunks_exact(4)
        .map(|d| {
            #[inline(always)]
            fn premultiply(e: u8, alpha: u16) -> u8 {
                ((e as u16 * alpha) / 255) as u8
            }
            let alpha = d[3] as u16;
            if alpha == 0 {
                PremulRgba8::from_u8_array([0, 0, 0, 0])
            } else {
                PremulRgba8 {
                    r: premultiply(d[0], alpha),
                    g: premultiply(d[1], alpha),
                    b: premultiply(d[2], alpha),
                    a: d[3],
                }
            }
        })
        .collect()
}

pub struct VelloCpuScenePainter(pub vello_cpu::RenderContext);

impl VelloCpuScenePainter {
    pub fn finish(self) -> Pixmap {
        let mut pixmap = Pixmap::new(self.0.width(), self.0.height());
        self.0
            .render_to_pixmap(&mut pixmap, RenderMode::OptimizeSpeed);
        pixmap
    }
}

impl PaintScene for VelloCpuScenePainter {
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
            Some(alpha),
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
        brush: impl Into<Paint<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        self.0.set_transform(transform);
        self.0.set_fill_rule(style);
        self.0
            .set_paint(anyrender_paint_to_vello_cpu_paint(brush.into()));
        self.0
            .set_paint_transform(brush_transform.unwrap_or(Affine::IDENTITY));
        self.0.fill_path(&shape.into_path(DEFAULT_TOLERANCE));
    }

    fn draw_glyphs<'a, 's: 'a>(
        &'a mut self,
        font: &'a Font,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [NormalizedCoord],
        style: impl Into<StyleRef<'a>>,
        brush: impl Into<BrushRef<'a>>,
        _brush_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = anyrender::Glyph>,
    ) {
        self.0.set_transform(transform);
        self.0.set_paint(brush_ref_to_paint_type(brush.into()));

        fn into_vello_cpu_glyph(g: anyrender::Glyph) -> vello_cpu::Glyph {
            vello_cpu::Glyph {
                id: g.id,
                x: g.x,
                y: g.y,
            }
        }

        let style: StyleRef<'a> = style.into();
        match style {
            StyleRef::Fill(fill) => {
                self.0.set_fill_rule(fill);
                self.0
                    .glyph_run(font)
                    .font_size(font_size)
                    .hint(hint)
                    .normalized_coords(normalized_coords)
                    .glyph_transform(glyph_transform.unwrap_or_default())
                    .fill_glyphs(glyphs.map(into_vello_cpu_glyph));
            }
            StyleRef::Stroke(stroke) => {
                self.0.set_stroke(stroke.clone());
                self.0
                    .glyph_run(font)
                    .font_size(font_size)
                    .hint(hint)
                    .normalized_coords(normalized_coords)
                    .glyph_transform(glyph_transform.unwrap_or_default())
                    .stroke_glyphs(glyphs.map(into_vello_cpu_glyph));
            }
        }
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
}
