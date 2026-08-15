use std::{any::Any, collections::HashSet, sync::Arc};

use anyrender::{
    Filter, Glyph, NormalizedCoord, PaintRef, PaintScene, RegisterResourceError, RenderContext,
    ResourceId, Scene,
};
use blitz_paint::{BlitzPaintScene, PaintNode, paint_scene, paint_scene_with_nodes};
use blitz_test_harness::Harness;
use kurbo::{Affine, Rect, Shape, Stroke, Vec2};
use peniko::{BlendMode, Color, Fill, FontData, StyleRef};

#[derive(Default)]
struct RecordingScene {
    inner: Scene,
    excluded: HashSet<PaintNode>,
    stack: Vec<PaintNode>,
    painted: Vec<PaintNode>,
}

impl RecordingScene {
    fn excluding(node: PaintNode) -> Self {
        Self {
            excluded: HashSet::from([node]),
            ..Self::default()
        }
    }
}

impl RenderContext for RecordingScene {
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

impl PaintScene for RecordingScene {
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
}

impl BlitzPaintScene for RecordingScene {
    fn should_paint(&self, node: PaintNode) -> bool {
        !self.excluded.contains(&node)
    }

    fn begin_node(&mut self, node: PaintNode) {
        self.stack.push(node);
        self.painted.push(node);
    }

    fn end_node(&mut self, node: PaintNode) {
        assert_eq!(self.stack.pop(), Some(node));
    }
}

#[test]
fn adapter_preserves_the_existing_paint_output() {
    let mut harness = Harness::from_html(
        r#"<body style="margin:0;background:white"><div style="width:20px;height:20px;background:red">text</div></body>"#,
    );

    let mut existing = Scene::new();
    paint_scene(&mut existing, &mut harness.base_mut(), 1.0, 100, 100, 0, 0);

    let mut node_aware = RecordingScene::default();
    paint_scene_with_nodes(
        &mut node_aware,
        &mut harness.base_mut(),
        1.0,
        100,
        100,
        0,
        0,
    );

    assert_eq!(node_aware.inner, existing);
    assert!(node_aware.stack.is_empty());
}

#[test]
fn selective_paint_skips_an_element_subtree() {
    let mut harness = Harness::from_html(
        r#"<body style="margin:0">
            <div id="keep" style="width:20px;height:20px;background:red"></div>
            <div id="skip" style="width:20px;height:20px;background:blue">
                <div id="descendant" style="width:10px;height:10px;background:green"></div>
            </div>
        </body>"#,
    );
    let skip = harness.node("#skip");
    let descendant = harness.node("#descendant");
    let document_id = harness.base().id();

    let mut full_scene = Scene::new();
    paint_scene(
        &mut full_scene,
        &mut harness.base_mut(),
        1.0,
        100,
        100,
        0,
        0,
    );

    let mut filtered = RecordingScene::excluding(PaintNode::new(document_id, skip));
    paint_scene_with_nodes(&mut filtered, &mut harness.base_mut(), 1.0, 100, 100, 0, 0);

    assert!(filtered.stack.is_empty());
    assert!(
        filtered
            .painted
            .iter()
            .all(|node| node.node_id != skip && node.node_id != descendant)
    );
    assert!(filtered.inner.commands.len() < full_scene.commands.len());
}

#[test]
fn inline_paint_scopes_use_the_inline_owner() {
    let mut harness = Harness::from_html(
        r#"<body><div>outside <span id="target" style="background:red">inside</span></div></body>"#,
    );
    let target = PaintNode::new(harness.base().id(), harness.node("#target"));

    let mut full = RecordingScene::default();
    paint_scene_with_nodes(&mut full, &mut harness.base_mut(), 1.0, 200, 100, 0, 0);

    assert!(full.stack.is_empty());
    assert!(full.painted.contains(&target));

    let mut filtered = RecordingScene::excluding(target);
    paint_scene_with_nodes(&mut filtered, &mut harness.base_mut(), 1.0, 200, 100, 0, 0);

    assert!(filtered.stack.is_empty());
    assert!(!filtered.painted.contains(&target));
    assert!(filtered.inner.commands.len() < full.inner.commands.len());
}

#[test]
fn propagated_body_background_keeps_the_body_owner() {
    let mut harness = Harness::from_html(r#"<body style="margin:0;background:red"></body>"#);
    let body = harness.node("body");
    let html = harness.node("html");
    let document_id = harness.base().id();

    let mut scene = RecordingScene::default();
    paint_scene_with_nodes(&mut scene, &mut harness.base_mut(), 1.0, 100, 100, 0, 0);

    assert_eq!(
        scene.painted.first(),
        Some(&PaintNode::new(document_id, body))
    );
    assert_ne!(
        scene.painted.first(),
        Some(&PaintNode::new(document_id, html))
    );
    assert!(scene.stack.is_empty());
}
