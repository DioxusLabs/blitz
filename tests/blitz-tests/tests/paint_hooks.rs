use std::collections::HashSet;

use anyrender::Scene;
use blitz_paint::{NoopPaintHooks, PaintHooks, PaintNode, paint_scene, paint_scene_with_hooks};
use blitz_test_harness::Harness;

#[derive(Default)]
struct RecordingHooks {
    excluded: HashSet<PaintNode>,
    stack: Vec<PaintNode>,
    painted: Vec<PaintNode>,
}

impl PaintHooks<Scene> for RecordingHooks {
    fn should_paint(&self, node: PaintNode) -> bool {
        !self.excluded.contains(&node)
    }

    fn begin_node(&mut self, _scene: &mut Scene, node: PaintNode) {
        self.stack.push(node);
        self.painted.push(node);
    }

    fn end_node(&mut self, _scene: &mut Scene, node: PaintNode) {
        assert_eq!(self.stack.pop(), Some(node));
    }
}

#[test]
fn no_op_hooks_preserve_the_existing_paint_output() {
    let mut harness = Harness::from_html(
        r#"<body style="margin:0;background:white"><div style="width:20px;height:20px;background:red">text</div></body>"#,
    );

    let mut existing = Scene::new();
    paint_scene(&mut existing, &mut harness.base_mut(), 1.0, 100, 100, 0, 0);

    let mut hooked = Scene::new();
    paint_scene_with_hooks(
        &mut hooked,
        &mut harness.base_mut(),
        1.0,
        100,
        100,
        0,
        0,
        &mut NoopPaintHooks,
    );

    assert_eq!(hooked, existing);
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

    let mut hooks = RecordingHooks {
        excluded: HashSet::from([PaintNode {
            document_id,
            node_id: skip,
        }]),
        ..Default::default()
    };
    let mut filtered_scene = Scene::new();
    paint_scene_with_hooks(
        &mut filtered_scene,
        &mut harness.base_mut(),
        1.0,
        100,
        100,
        0,
        0,
        &mut hooks,
    );

    assert!(hooks.stack.is_empty());
    assert!(
        hooks
            .painted
            .iter()
            .all(|node| node.node_id != skip && node.node_id != descendant)
    );
    assert!(filtered_scene.commands.len() < full_scene.commands.len());
}

#[test]
fn inline_paint_scopes_use_the_inline_owner() {
    let mut harness = Harness::from_html(
        r#"<body><div>outside <span id="target" style="background:red">inside</span></div></body>"#,
    );
    let target = harness.node("#target");
    let document_id = harness.base().id();
    let target = PaintNode {
        document_id,
        node_id: target,
    };

    let mut hooks = RecordingHooks::default();
    let mut full_scene = Scene::new();
    paint_scene_with_hooks(
        &mut full_scene,
        &mut harness.base_mut(),
        1.0,
        200,
        100,
        0,
        0,
        &mut hooks,
    );

    assert!(hooks.stack.is_empty());
    assert!(hooks.painted.contains(&target));

    let mut filtered_hooks = RecordingHooks {
        excluded: HashSet::from([target]),
        ..Default::default()
    };
    let mut filtered_scene = Scene::new();
    paint_scene_with_hooks(
        &mut filtered_scene,
        &mut harness.base_mut(),
        1.0,
        200,
        100,
        0,
        0,
        &mut filtered_hooks,
    );

    assert!(filtered_hooks.stack.is_empty());
    assert!(!filtered_hooks.painted.contains(&target));
    assert!(filtered_scene.commands.len() < full_scene.commands.len());
}

#[test]
fn propagated_body_background_keeps_the_body_owner() {
    let mut harness = Harness::from_html(r#"<body style="margin:0;background:red"></body>"#);
    let body = harness.node("body");
    let html = harness.node("html");
    let document_id = harness.base().id();

    let mut hooks = RecordingHooks::default();
    let mut scene = Scene::new();
    paint_scene_with_hooks(
        &mut scene,
        &mut harness.base_mut(),
        1.0,
        100,
        100,
        0,
        0,
        &mut hooks,
    );

    assert_eq!(
        hooks.painted.first(),
        Some(&PaintNode::new(document_id, body))
    );
    assert_ne!(
        hooks.painted.first(),
        Some(&PaintNode::new(document_id, html))
    );
    assert!(hooks.stack.is_empty());
}
