//! Types that are used within the Anyrender traits

use peniko::{BrushRef, Color, Gradient, ImageBrushRef};
use std::{any::Any, sync::Arc};

pub type NormalizedCoord = i16;

/// A positioned glyph.
#[derive(Copy, Clone, Debug)]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Copy, Clone, Debug)]
pub struct CustomPaint {
    pub source_id: u64,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

#[derive(Clone, Debug)]
pub enum Paint<'a> {
    /// Solid color brush.
    Solid(Color),
    /// Gradient brush.
    Gradient(&'a Gradient),
    /// Image brush.
    Image(ImageBrushRef<'a>),
    /// Custom paint (type erased as each backend will have their own)
    Custom(Arc<dyn Any + Send + Sync>),
}
impl From<Color> for Paint<'_> {
    fn from(value: Color) -> Self {
        Paint::Solid(value)
    }
}
impl<'a> From<&'a Gradient> for Paint<'a> {
    fn from(value: &'a Gradient) -> Self {
        Paint::Gradient(value)
    }
}
impl<'a> From<ImageBrushRef<'a>> for Paint<'a> {
    fn from(value: ImageBrushRef<'a>) -> Self {
        Paint::Image(value)
    }
}
impl<'a> From<Arc<dyn Any + Send + Sync>> for Paint<'a> {
    fn from(value: Arc<dyn Any + Send + Sync>) -> Self {
        Paint::Custom(value)
    }
}
impl<'a> From<BrushRef<'a>> for Paint<'a> {
    fn from(value: BrushRef<'a>) -> Self {
        match value {
            BrushRef::Solid(color) => Paint::Solid(color),
            BrushRef::Gradient(gradient) => Paint::Gradient(gradient),
            BrushRef::Image(image) => Paint::Image(image),
        }
    }
}
