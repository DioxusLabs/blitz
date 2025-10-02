//! 2D drawing abstraction that allows applications/frameworks to support many rendering backends through
//! a unified API.
//!
//! ### Painting a scene
//!
//! The core abstraction in Anyrenderis the [`PaintScene`] trait.
//!
//! [`PaintScene`] is a "sink" which accepts drawing commands:
//!
//!   - Applications and libraries draw by pushing commands into a [`PaintScene`]
//!   - Backends execute those commands to produce an output
//!
//! ### Rendering to surface or buffer
//!
//! In addition to PaintScene, there is:
//!
//!   - The [`ImageRenderer`] trait which provides an abstraction for rendering to a `Vec<u8>` RGBA8 buffer.
//!   - The [`WindowRenderer`] trait which provides an abstraction for rendering to a surface/window
//!
//! ### SVG
//!
//! The [anyrender_svg](https://docs.rs/anyrender_svg) crate allows SVGs to be rendered using Anyrender
//!
//! ### Backends
//!
//! Currently existing backends are:
//!  - [anyrender_vello](https://docs.rs/anyrender_vello)
//!  - [anyrender_vello_cpu](https://docs.rs/anyrender_vello_cpu)

use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, BrushRef, Color, Fill, FontData, ImageBrushRef, StyleRef};
use std::sync::Arc;

pub mod wasm_send_sync;
pub use wasm_send_sync::*;
pub mod types;
pub use types::*;

/// Abstraction for rendering a scene to a window
pub trait WindowRenderer {
    type ScenePainter<'a>: PaintScene
    where
        Self: 'a;
    fn resume(&mut self, window: Arc<dyn WindowHandle>, width: u32, height: u32);
    fn suspend(&mut self);
    fn is_active(&self) -> bool;
    fn set_size(&mut self, width: u32, height: u32);
    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F);
}

/// Abstraction for rendering a scene to an image buffer
pub trait ImageRenderer {
    type ScenePainter<'a>: PaintScene
    where
        Self: 'a;
    fn new(width: u32, height: u32) -> Self;
    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F, buffer: &mut Vec<u8>);
}

/// Draw a scene to a buffer using an `ImageRenderer`
pub fn render_to_buffer<R: ImageRenderer, F: FnOnce(&mut R::ScenePainter<'_>)>(
    draw_fn: F,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity((width * height * 4) as usize);
    let mut renderer = R::new(width, height);
    renderer.render(draw_fn, &mut buf);

    buf
}

/// Abstraction for drawing a 2D scene
pub trait PaintScene {
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
        brush: impl Into<Paint<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    );

    /// Returns a builder for encoding a glyph run.
    #[allow(clippy::too_many_arguments)]
    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
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

    // --- Provided methods

    /// Utility method to draw an image at it's natural size. For more advanced image drawing use the `fill` method
    fn draw_image(&mut self, image: ImageBrushRef, transform: Affine) {
        self.fill(
            Fill::NonZero,
            transform,
            image,
            None,
            &Rect::new(
                0.0,
                0.0,
                image.image.width as f64,
                image.image.height as f64,
            ),
        );
    }
}
