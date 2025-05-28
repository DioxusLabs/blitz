use std::collections::HashMap;

use crate::{Glyph, NormalizedCoord, Paint, PaintScene};
use kurbo::{Affine, BezPath, Rect, Shape, Stroke};
use peniko::{BlendMode, Blob, BrushRef, Color, Fill, FontData, Gradient, Style, StyleRef};

const DEFAULT_TOLERANCE: f64 = 0.1;

#[derive(Clone)]
pub struct ResourceId(pub u64);

#[derive(Clone)]
pub struct Resource {
    pub blob: Blob<u8>,
}

#[derive(Clone)]
pub enum RenderCommand {
    PushLayer(LayerCmd),
    PopLayer,
    Stroke(StrokeCmd),
    Fill(FillCmd),
    GlyphRun(GlyphRunCmd),
    BoxShadow(BoxShadowCmd),
}

#[derive(Clone)]
pub enum RecordedPaint {
    /// Solid color brush.
    Solid(Color),
    /// Gradient brush.
    Gradient(Gradient),
    /// Image brush.
    Image(ResourceId),
}

#[derive(Clone)]
pub struct LayerCmd {
    pub blend: BlendMode,
    pub alpha: f32,
    pub transform: Affine,
    pub clip: BezPath, // TODO: more shape options
}

#[derive(Clone)]
pub struct StrokeCmd {
    pub style: Stroke,
    pub transform: Affine,
    pub brush: RecordedPaint, // TODO: review ownership to avoid cloning. Should brushes be a "resource"?
    pub brush_transform: Option<Affine>,
    pub shape: BezPath, // TODO: more shape options
}

#[derive(Clone)]
pub struct FillCmd {
    pub fill: Fill,
    pub transform: Affine,
    pub brush: RecordedPaint, // TODO: review ownership to avoid cloning. Should brushes be a "resource"?
    pub brush_transform: Option<Affine>,
    pub shape: BezPath, // TODO: more shape options
}

#[derive(Clone)]
pub struct GlyphRunCmd {
    pub font_data: ResourceId,
    pub font_index: u32,
    pub font_size: f32,
    pub hint: bool,
    pub normalized_coords: Vec<NormalizedCoord>,
    pub style: Style,
    pub brush: RecordedPaint,
    pub brush_alpha: f32,
    pub transform: Affine,
    pub glyph_transform: Option<Affine>,
    pub glyphs: Vec<Glyph>,
}

#[derive(Clone)]
pub struct BoxShadowCmd {
    pub transform: Affine,
    pub rect: Rect,
    pub brush: Color,
    pub radius: f64,
    pub std_dev: f64,
}

pub struct Recording {
    pub tolerance: f64,
    pub resources: HashMap<u64, Resource>,
    pub cmds: Vec<RenderCommand>,
}

impl Default for Recording {
    fn default() -> Self {
        Self {
            tolerance: DEFAULT_TOLERANCE,
            resources: HashMap::new(),
            cmds: Vec::new(),
        }
    }
}

impl Recording {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tolerance(tolerance: f64) -> Self {
        Self {
            tolerance,
            resources: HashMap::new(),
            cmds: Vec::new(),
        }
    }

    pub fn store_resource(&mut self, blob: Blob<u8>) -> ResourceId {
        let id = blob.id();
        self.resources.entry(id).or_insert(Resource { blob });
        ResourceId(id)
    }

    pub fn store_resource_ref(&mut self, blob: &Blob<u8>) -> ResourceId {
        let id = blob.id();
        self.resources
            .entry(id)
            .or_insert_with(|| Resource { blob: blob.clone() });
        ResourceId(id)
    }

    pub fn convert_brushref(&mut self, brush_ref: BrushRef<'_>) -> RecordedPaint {
        match brush_ref {
            BrushRef::Solid(color) => RecordedPaint::Solid(color),
            BrushRef::Gradient(gradient) => RecordedPaint::Gradient(gradient.clone()),
            BrushRef::Image(image) => {
                let id = self.store_resource_ref(&image.image.data);
                RecordedPaint::Image(id)
            }
        }
    }

    pub fn convert_paintref(&mut self, paint_ref: Paint<'_>) -> RecordedPaint {
        match paint_ref {
            Paint::Solid(color) => RecordedPaint::Solid(color),
            Paint::Gradient(gradient) => RecordedPaint::Gradient(gradient.clone()),
            Paint::Image(image) => {
                let id = self.store_resource_ref(&image.image.data);
                RecordedPaint::Image(id)
            }
            // TODO: handle this somehow
            Paint::Custom(_) => RecordedPaint::Solid(Color::TRANSPARENT),
        }
    }
}

impl PaintScene for Recording {
    fn reset(&mut self) {
        self.cmds.clear()
    }

    fn push_layer(
        &mut self,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
    ) {
        let blend = blend.into();
        let clip = clip.into_path(self.tolerance);
        let layer = LayerCmd {
            blend,
            alpha,
            transform,
            clip,
        };
        self.cmds.push(RenderCommand::PushLayer(layer));
    }

    fn pop_layer(&mut self) {
        self.cmds.push(RenderCommand::PopLayer);
    }

    fn stroke<'a>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        brush: impl Into<BrushRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let shape = shape.into_path(self.tolerance);
        let brush = self.convert_brushref(brush.into());
        let stroke = StrokeCmd {
            style: style.clone(),
            transform,
            brush,
            brush_transform,
            shape,
        };
        self.cmds.push(RenderCommand::Stroke(stroke));
    }

    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        paint: impl Into<Paint<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let shape = shape.into_path(self.tolerance);
        let brush = self.convert_paintref(paint.into());
        let fill = FillCmd {
            fill: style,
            transform,
            brush,
            brush_transform,
            shape,
        };
        self.cmds.push(RenderCommand::Fill(fill));
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
        glyphs: impl Iterator<Item = Glyph>,
    ) {
        let font_index = font.index;
        let font_data = self.store_resource_ref(&font.data);
        let brush = self.convert_brushref(brush.into());
        let glyph_run = GlyphRunCmd {
            font_data,
            font_index,
            font_size,
            hint,
            normalized_coords: normalized_coords.to_vec(),
            style: style.into().to_owned(),
            brush,
            brush_alpha,
            transform,
            glyph_transform,
            glyphs: glyphs.into_iter().collect(),
        };
        self.cmds.push(RenderCommand::GlyphRun(glyph_run));
    }

    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        brush: Color,
        radius: f64,
        std_dev: f64,
    ) {
        let box_shadow = BoxShadowCmd {
            transform,
            rect,
            brush,
            radius,
            std_dev,
        };
        self.cmds.push(RenderCommand::BoxShadow(box_shadow));
    }
}
