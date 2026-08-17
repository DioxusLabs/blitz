//! Element coordinates of pointer events must be relative to the target's
//! border box in its own (transformed) coordinate space: a CSS transform on an
//! element (or an ancestor) moves the element on screen, so the point must be
//! mapped through the inverse transform, not just offset by the untransformed
//! layout position.
//!
//! Regression test for https://github.com/DioxusLabs/blitz/issues/663

use blitz_dom::{Document, EventDriver, EventHandler};
use blitz_test_harness::{Harness, HarnessOptions, mouse_pointer_event};
use blitz_traits::events::{DomEvent, DomEventData, EventState, UiEvent};
use blitz_traits::node_id::NodeId;
use std::cell::RefCell;
use std::rc::Rc;

/// Dispatch a pointerdown at page coordinates `(x, y)`, returning the
/// (target, element coordinates) of the dispatched `pointerdown` event.
fn pointer_down_element_coords(harness: &mut Harness, x: f32, y: f32) -> (NodeId, (f32, f32)) {
    #[derive(Clone, Default)]
    struct RecordingHandler {
        result: Rc<RefCell<Option<(NodeId, (f32, f32))>>>,
    }

    impl EventHandler for RecordingHandler {
        fn handle_event(
            &mut self,
            _chain: &[NodeId],
            event: &mut DomEvent,
            _doc: &mut dyn blitz_dom::Document,
            _event_state: &mut EventState,
        ) {
            if let DomEventData::PointerDown(data) = &event.data {
                *self.result.borrow_mut() =
                    Some((event.target, (data.element_x(), data.element_y())));
            }
        }
    }

    let handler = RecordingHandler::default();
    let result = handler.result.clone();
    let mut doc = harness.doc.inner_mut();
    let mut driver = EventDriver::new(&mut *doc, handler);
    driver.handle_ui_event(UiEvent::PointerDown(mouse_pointer_event(x, y)));
    drop(doc);
    let result = result.borrow().expect("no pointerdown event dispatched");
    result
}

const TRANSLATED_HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    * { box-sizing: border-box; }
    body { margin: 0; }
</style></head>
<body>
    <div id="parent" style="position: relative; transform: translate(50px, 20px); width: 300px; height: 300px; border: 1px solid blue">
        <div id="child" style="position: absolute; top: 10px; left: 10px; width: 200px; height: 200px; border: 1px solid red"></div>
    </div>
</body></html>
"#;

#[test]
fn element_coords_account_for_ancestor_translation() {
    let mut harness = Harness::from_html(TRANSLATED_HTML);

    // #child's border box is at (50+1+10, 20+1+10) = (61, 31) in page coordinates
    let child = harness.node("#child");
    let (target, (x, y)) = pointer_down_element_coords(&mut harness, 75.0, 45.0);
    assert_eq!(target, child);
    assert!(
        (x - 14.0).abs() < 0.01 && (y - 14.0).abs() < 0.01,
        "expected element coords (14, 14), got ({x}, {y})"
    );
}

#[test]
fn element_coords_account_for_own_translation() {
    let mut harness = Harness::from_html(TRANSLATED_HTML);

    // Hit #parent itself (its border box is at (50, 20) in page coordinates),
    // in the strip between its border and #child
    let parent = harness.node("#parent");
    let (target, (x, y)) = pointer_down_element_coords(&mut harness, 55.0, 25.0);
    assert_eq!(target, parent);
    assert!(
        (x - 5.0).abs() < 0.01 && (y - 5.0).abs() < 0.01,
        "expected element coords (5, 5), got ({x}, {y})"
    );
}

#[test]
fn element_coords_account_for_translation_at_hidpi_scale() {
    let mut harness = Harness::from_html_with(
        TRANSLATED_HTML,
        HarnessOptions {
            scale: 2.0,
            ..Default::default()
        },
    );

    let child = harness.node("#child");
    let (target, (x, y)) = pointer_down_element_coords(&mut harness, 75.0, 45.0);
    assert_eq!(target, child);
    assert!(
        (x - 14.0).abs() < 0.01 && (y - 14.0).abs() < 0.01,
        "expected element coords (14, 14), got ({x}, {y})"
    );
}

#[test]
fn element_coords_account_for_rotation() {
    let mut harness = Harness::from_html(
        r#"<!DOCTYPE html>
<html><head><style>body { margin: 0; }</style></head>
<body>
    <div id="rotated" style="width: 100px; height: 100px; transform: rotate(90deg)"></div>
</body></html>
"#,
    );

    // The point (50, 10) maps to (10, 50) in the element's local space after
    // inverting the 90deg rotation about the element's center (50, 50)
    let rotated = harness.node("#rotated");
    let (target, (x, y)) = pointer_down_element_coords(&mut harness, 50.0, 10.0);
    assert_eq!(target, rotated);
    assert!(
        (x - 10.0).abs() < 0.01 && (y - 50.0).abs() < 0.01,
        "expected element coords (10, 50), got ({x}, {y})"
    );
}

#[test]
fn element_coords_unchanged_without_transform() {
    let mut harness = Harness::from_html(
        r#"<!DOCTYPE html>
<html><head><style>
    * { box-sizing: border-box; }
    body { margin: 0; }
</style></head>
<body>
    <div id="parent" style="position: relative; width: 300px; height: 300px; border: 1px solid blue">
        <div id="child" style="position: absolute; top: 10px; left: 10px; width: 200px; height: 200px; border: 1px solid red"></div>
    </div>
</body></html>
"#,
    );

    // #child's border box is at (11, 11) in page coordinates
    let child = harness.node("#child");
    let (target, (x, y)) = pointer_down_element_coords(&mut harness, 25.0, 35.0);
    assert_eq!(target, child);
    assert!(
        (x - 14.0).abs() < 0.01 && (y - 24.0).abs() < 0.01,
        "expected element coords (14, 24), got ({x}, {y})"
    );
}
