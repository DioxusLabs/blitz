//! Tests for structured event tracing via `Harness::dispatch_traced`.

use blitz_test_harness::{Harness, mouse_pointer_event};
use blitz_traits::events::UiEvent;

#[test]
fn dispatch_traced_captures_names_and_targets() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <button id="btn" style="width:100px; height:30px;">Click me</button>
        </body></html>"#,
    );
    let button = harness.node("#btn");
    let (x, y) = harness.center_of("#btn");

    let event = mouse_pointer_event(x, y);
    let traced = harness.dispatch_traced([
        UiEvent::PointerDown(event.clone()),
        UiEvent::PointerUp(event),
    ]);

    let names: Vec<&str> = traced.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"mousedown"), "traced: {names:?}");
    assert!(names.contains(&"mouseup"), "traced: {names:?}");
    assert!(names.contains(&"click"), "traced: {names:?}");

    let click = traced.iter().find(|e| e.name == "click").unwrap();
    assert_eq!(click.target, button);
}
