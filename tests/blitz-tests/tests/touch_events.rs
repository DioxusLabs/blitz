//! Touch events (touchstart, touchmove, touchend) are generated from finger
//! pointer input and dispatched to application code alongside the corresponding
//! pointer events. Mouse input must NOT generate touch events (and vice-versa),
//! and default actions remain driven by the pointer events.

use blitz_test_harness::{Harness, HarnessOptions, pointer_event};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, UiEvent,
};
use keyboard_types::Modifiers;

fn event(id: BlitzPointerId, x: f32, y: f32) -> BlitzPointerEvent {
    pointer_event(
        id,
        x,
        y,
        MouseEventButton::Main,
        MouseEventButtons::from(MouseEventButton::Main),
        Modifiers::default(),
    )
}

fn target_harness() -> Harness {
    Harness::from_html_with(
        r#"<html><body style="margin:0">
        <div id="target" style="width:200px; height:200px;"></div>
    </body></html>"#,
        HarnessOptions {
            width: 200,
            height: 200,
            ..Default::default()
        },
    )
}

#[test]
fn finger_input_generates_touch_events() {
    let mut harness = target_harness();
    let finger = BlitzPointerId::Finger(0);
    let names = harness.dispatch_recorded([
        UiEvent::PointerDown(event(finger, 50.0, 50.0)),
        UiEvent::PointerMove(event(finger, 60.0, 60.0)),
        UiEvent::PointerUp(event(finger, 60.0, 60.0)),
    ]);

    assert!(
        names.contains(&"touchstart".to_string()),
        "expected a touchstart event, got {names:?}"
    );
    assert!(
        names.contains(&"touchmove".to_string()),
        "expected a touchmove event, got {names:?}"
    );
    assert!(
        names.contains(&"touchend".to_string()),
        "expected a touchend event, got {names:?}"
    );

    // The pointer events must still be dispatched.
    assert!(names.contains(&"pointerdown".to_string()));
    assert!(names.contains(&"pointermove".to_string()));
    assert!(names.contains(&"pointerup".to_string()));

    // Finger input must not generate mouse compatibility events.
    assert!(
        !names.iter().any(|n| n.starts_with("mouse")),
        "finger input should not generate mouse events, got {names:?}"
    );
}

#[test]
fn pen_input_generates_touch_events() {
    let mut harness = target_harness();
    let pen = BlitzPointerId::Pen;
    let names = harness.dispatch_recorded([
        UiEvent::PointerDown(event(pen, 50.0, 50.0)),
        UiEvent::PointerMove(event(pen, 60.0, 60.0)),
        UiEvent::PointerUp(event(pen, 60.0, 60.0)),
    ]);

    assert!(
        names.contains(&"touchstart".to_string()),
        "expected a touchstart event, got {names:?}"
    );
    assert!(
        names.contains(&"touchmove".to_string()),
        "expected a touchmove event, got {names:?}"
    );
    assert!(
        names.contains(&"touchend".to_string()),
        "expected a touchend event, got {names:?}"
    );

    // The pointer events must still be dispatched.
    assert!(names.contains(&"pointerdown".to_string()));

    // Pen input must not generate mouse compatibility events.
    assert!(
        !names.iter().any(|n| n.starts_with("mouse")),
        "pen input should not generate mouse events, got {names:?}"
    );
}

#[test]
fn finger_cancel_generates_pointercancel_and_touchcancel() {
    let mut harness = target_harness();
    let finger = BlitzPointerId::Finger(0);
    let names = harness.dispatch_recorded([
        UiEvent::PointerDown(event(finger, 50.0, 50.0)),
        UiEvent::PointerCancel(event(finger, 50.0, 50.0)),
    ]);

    assert!(
        names.contains(&"pointercancel".to_string()),
        "expected a pointercancel event, got {names:?}"
    );
    assert!(
        names.contains(&"touchcancel".to_string()),
        "expected a touchcancel event, got {names:?}"
    );

    // A cancelled interaction must not produce up/end events.
    assert!(
        !names.contains(&"pointerup".to_string()),
        "cancel should not produce pointerup, got {names:?}"
    );
    assert!(
        !names.contains(&"touchend".to_string()),
        "cancel should not produce touchend, got {names:?}"
    );

    // Finger input must not generate mouse compatibility events.
    assert!(
        !names.iter().any(|n| n.starts_with("mouse")),
        "finger input should not generate mouse events, got {names:?}"
    );
}

#[test]
fn pen_cancel_generates_pointercancel_and_touchcancel() {
    let mut harness = target_harness();
    let pen = BlitzPointerId::Pen;
    let names = harness.dispatch_recorded([
        UiEvent::PointerDown(event(pen, 50.0, 50.0)),
        UiEvent::PointerCancel(event(pen, 50.0, 50.0)),
    ]);

    assert!(
        names.contains(&"pointercancel".to_string()),
        "expected a pointercancel event, got {names:?}"
    );
    assert!(
        names.contains(&"touchcancel".to_string()),
        "expected a touchcancel event, got {names:?}"
    );
}

#[test]
fn mouse_cancel_generates_pointercancel_without_touch_or_mouse() {
    let mut harness = target_harness();
    let mouse = BlitzPointerId::Mouse;
    let names = harness.dispatch_recorded([
        UiEvent::PointerDown(event(mouse, 50.0, 50.0)),
        UiEvent::PointerCancel(event(mouse, 50.0, 50.0)),
    ]);

    assert!(
        names.contains(&"pointercancel".to_string()),
        "expected a pointercancel event, got {names:?}"
    );
    // Mouse input has no touchcancel and no mouse-cancel compatibility event.
    assert!(
        !names.iter().any(|n| n.starts_with("touch")),
        "mouse input should not generate touch events, got {names:?}"
    );
    assert!(
        !names.contains(&"mousecancel".to_string()),
        "there is no mousecancel event, got {names:?}"
    );
}

#[test]
fn mouse_input_does_not_generate_touch_events() {
    let mut harness = target_harness();
    let mouse = BlitzPointerId::Mouse;
    let names = harness.dispatch_recorded([
        UiEvent::PointerDown(event(mouse, 50.0, 50.0)),
        UiEvent::PointerMove(event(mouse, 60.0, 60.0)),
        UiEvent::PointerUp(event(mouse, 60.0, 60.0)),
    ]);

    assert!(
        !names.iter().any(|n| n.starts_with("touch")),
        "mouse input should not generate touch events, got {names:?}"
    );
    // Mouse compatibility events are still generated.
    assert!(names.contains(&"mousedown".to_string()));
}
