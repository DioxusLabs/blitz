use std::collections::HashMap;

use crate::{Glyph, NormalizedCoord, Scene};
use kurbo::{Affine, BezPath, Rect, Shape, Stroke};
use peniko::{BlendMode, Blob, BrushRef, Color, Fill, Font, Gradient, Style, StyleRef};

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
pub enum Paint {
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
    pub brush: Paint, // TODO: review ownership to avoid cloning. Should brushes be a "resource"?
    pub brush_transform: Option<Affine>,
    pub shape: BezPath, // TODO: more shape options
}

#[derive(Clone)]
pub struct FillCmd {
    pub fill: Fill,
    pub transform: Affine,
    pub brush: Paint, // TODO: review ownership to avoid cloning. Should brushes be a "resource"?
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
    pub brush: Paint,
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

    pub fn convert_brush(&mut self, brush_ref: BrushRef<'_>) -> Paint {
        match brush_ref {
            BrushRef::Solid(color) => Paint::Solid(color),
            BrushRef::Gradient(gradient) => Paint::Gradient(gradient.clone()),
            BrushRef::Image(image) => {
                let id = self.store_resource_ref(&image.data);
                Paint::Image(id)
            }
        }
    }
}

impl Scene for Recording {
    type Output = Self;

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
        let brush = self.convert_brush(brush.into());
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
        brush: impl Into<BrushRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let shape = shape.into_path(self.tolerance);
        let brush = self.convert_brush(brush.into());
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
        font: &'a Font,
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
        let brush = self.convert_brush(brush.into());
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

    fn finish(self) -> Self::Output {
        self
    }
}
