use std::{any::Any, sync::Arc};

use anyrender::{
    Filter, Glyph, NormalizedCoord, PaintRef, PaintScene, RegisterResourceError, RenderContext,
    ResourceId, Scene,
};
use blitz_dom::NodeId;
use kurbo::{Affine, Rect, Shape, Stroke, Vec2};
use peniko::{BlendMode, Color, Fill, FontData, ImageBrushRef, StyleRef};

/// Identifies the DOM node that owns a contiguous group of paint commands.
///
/// Node IDs are local to a document, so subdocuments are distinguished by
/// their document ID.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PaintNode {
    pub document_id: usize,
    pub node_id: NodeId,
}

impl PaintNode {
    /// Creates an identifier from a [`blitz_dom::BaseDocument::id`] and node ID.
    pub const fn new(document_id: usize, node_id: NodeId) -> Self {
        Self {
            document_id,
            node_id,
        }
    }
}

/// An AnyRender scene that can select and identify Blitz DOM paint output.
///
/// A node may produce more than one scope because its commands need not be
/// contiguous in CSS paint order. Scopes are properly nested and every
/// `begin_node` call is followed by `end_node`. A scope can be empty when an
/// element has no visible paint of its own.
pub trait BlitzPaintScene: PaintScene {
    /// Returns whether paint owned by this node should be emitted.
    ///
    /// Returning `false` stops that paint traversal branch. Callers selecting
    /// individual descendants must therefore retain their ancestor chain.
    /// Independently painted owners, including hoisted descendants and inline
    /// text runs, are checked separately. This method may be called more than
    /// once for a node and must return a consistent result during one paint
    /// operation.
    #[inline]
    fn should_paint(&self, _node: PaintNode) -> bool {
        true
    }

    /// Called immediately before a contiguous group of commands owned by a node.
    #[inline]
    fn begin_node(&mut self, _node: PaintNode) {}

    /// Called immediately after a contiguous group of commands owned by a node.
    #[inline]
    fn end_node(&mut self, _node: PaintNode) {}
}

/// Adapts an ordinary AnyRender scene by ignoring Blitz node metadata.
pub struct PaintSceneAdapter<'a, S: PaintScene> {
    inner: &'a mut S,
}

impl<'a, S: PaintScene> PaintSceneAdapter<'a, S> {
    /// Creates an adapter that forwards drawing commands to `inner`.
    pub fn new(inner: &'a mut S) -> Self {
        Self { inner }
    }
}

impl<S: PaintScene> RenderContext for PaintSceneAdapter<'_, S> {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn Any>,
    ) -> Result<ResourceId, RegisterResourceError> {
        self.inner.try_register_custom_resource(resource)
    }

    fn unregister_resource(&mut self, resource_id: ResourceId) {
        self.inner.unregister_resource(resource_id);
    }

    fn renderer_specific_context(&self) -> Option<Box<dyn Any>> {
        self.inner.renderer_specific_context()
    }
}

impl<S: PaintScene> PaintScene for PaintSceneAdapter<'_, S> {
    fn reset(&mut self) {
        self.inner.reset();
    }

    fn push_layer(
        &mut self,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) {
        self.inner
            .push_layer(blend, alpha, transform, clip, filter, backdrop_filter);
    }

    fn push_clip_layer(&mut self, transform: Affine, clip: &impl Shape) {
        self.inner.push_clip_layer(transform, clip);
    }

    fn pop_layer(&mut self) {
        self.inner.pop_layer();
    }

    fn stroke<'a>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        brush: impl Into<PaintRef<'a>>,
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
        brush: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        self.inner
            .fill(style, transform, brush, brush_transform, shape);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
        font: &'a FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [NormalizedCoord],
        embolden: Vec2,
        style: impl Into<StyleRef<'a>>,
        brush: impl Into<PaintRef<'a>>,
        brush_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = Glyph> + Clone,
    ) {
        self.inner.draw_glyphs(
            font,
            font_size,
            hint,
            normalized_coords,
            embolden,
            style,
            brush,
            brush_alpha,
            transform,
            glyph_transform,
            glyphs,
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
            .draw_box_shadow(transform, rect, brush, radius, std_dev);
    }

    fn append_scene(&mut self, scene: Scene, scene_transform: Affine) {
        self.inner.append_scene(scene, scene_transform);
    }

    fn draw_image(&mut self, image: ImageBrushRef, transform: Affine) {
        self.inner.draw_image(image, transform);
    }
}

impl<S: PaintScene> BlitzPaintScene for PaintSceneAdapter<'_, S> {}

#[inline]
pub(crate) fn with_node<S, F>(scene: &mut S, node: PaintNode, paint: F)
where
    S: BlitzPaintScene,
    F: FnOnce(&mut S),
{
    scene.begin_node(node);
    paint(scene);
    scene.end_node(node);
}
