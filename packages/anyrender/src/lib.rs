use std::sync::Arc;

use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, BrushRef, Color, Fill, Font, Image, StyleRef};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

mod wasm_send_sync;
pub use wasm_send_sync::*;

pub type NormalizedCoord = i16;

/// A positioned glyph.
#[derive(Copy, Clone, Debug)]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

// #[derive(Copy, Clone, Debug)]
// pub struct Viewport {
//     pub width: u32,
//     pub height: u32,
//     pub scale: f64,
// }

pub trait WindowHandle: HasWindowHandle + HasDisplayHandle + WasmNotSendSync {}
impl<T: HasWindowHandle + HasDisplayHandle + WasmNotSendSync> WindowHandle for T {}

pub trait WindowRenderer {
    type Scene: Scene;
    fn new(window: Arc<dyn WindowHandle>) -> Self;
    fn resume(&mut self, width: u32, height: u32);
    fn suspend(&mut self);
    fn is_active(&self) -> bool;
    fn set_size(&mut self, width: u32, height: u32);
    fn render<F: FnOnce(&mut Self::Scene)>(&mut self, draw_fn: F);
}

pub trait ImageRenderer {
    type Scene: Scene;
    fn new(width: u32, height: u32) -> Self;
    fn render<F: FnOnce(&mut Self::Scene)>(&mut self, draw_fn: F, buffer: &mut Vec<u8>);
}

pub fn render_to_buffer<R: ImageRenderer, F: FnOnce(&mut R::Scene)>(
    draw_fn: F,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity((width * height * 4) as usize);
    let mut renderer = R::new(width, height);
    renderer.render(draw_fn, &mut buf);

    buf
}

/// The primary drawing abstraction for drawing a single 2D scene
pub trait Scene {
    /// The output type.
    /// This will usually be either a rendered scene or an encoded set of instructions with which to render a scene.
    type Output: 'static;

    /// Removes all content from the scene
    fn reset(&mut self);

    /// Pushes a new layer clipped by the specified shape and composed with previous layers using the specified blend mode.
    /// Every drawing command after this call will be clipped by the shape until the layer is popped.
    /// However, the transforms are not saved or modified by the layer stack.
    fn push_layer(
        &mut self,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
    );

    /// Pops the current layer.
    fn pop_layer(&mut self);

    /// Strokes a shape using the specified style and brush.
    fn stroke<'a>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        brush: impl Into<BrushRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    );

    /// Fills a shape using the specified style and brush.
    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        brush: impl Into<BrushRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    );

    /// Returns a builder for encoding a glyph run.
    #[allow(clippy::too_many_arguments)]
    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
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
    );

    /// Draw a rounded rectangle blurred with a gaussian filter.
    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        brush: Color,
        radius: f64,
        std_dev: f64,
    );

    /// Turn the scene into it's output type.
    fn finish(self) -> Self::Output;

    // --- Provided methods

    /// Utility method to draw an image at it's natural size. For more advanced image drawing use the `fill` method
    fn draw_image(&mut self, image: &Image, transform: Affine) {
        self.fill(
            Fill::NonZero,
            transform,
            image,
            None,
            &Rect::new(0.0, 0.0, image.width as f64, image.height as f64),
        );
    }
}
