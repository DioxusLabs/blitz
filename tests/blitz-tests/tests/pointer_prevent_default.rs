//! Cancelling a pointer event takes away the compatibility mouse events, but
//! not the activation: `click` fires either way (Pointer Events, "Compatibility
//! mapping with mouse events").

use blitz_dom::{Document, EventDriver, EventHandler};
use blitz_test_harness::{Harness, HarnessOptions, pointer_event};
use blitz_traits::events::{
    BlitzPointerId, DomEvent, EventState, MouseEventButton, MouseEventButtons, UiEvent,
};
use blitz_traits::node_id::NodeId;
use keyboard_types::Modifiers;

/// Records every dispatched event and cancels the ones it is told to, the way
/// a page's own handler does.
struct CancellingHandler {
    cancel: &'static str,
    seen: Vec<String>,
}

impl EventHandler for &mut CancellingHandler {
    fn handle_event(
        &mut self,
        _chain: &[NodeId],
        event: &mut DomEvent,
        _doc: &mut dyn Document,
        event_state: &mut EventState,
    ) {
        self.seen.push(event.name().to_string());
        if event.name() == self.cancel {
            event_state.prevent_default();
        }
    }
}

fn press(x: f32, y: f32) -> blitz_traits::events::BlitzPointerEvent {
    pointer_event(
        BlitzPointerId::Mouse,
        x,
        y,
        MouseEventButton::Main,
        MouseEventButtons::from(MouseEventButton::Main),
        Modifiers::default(),
    )
}

/// Press and release a button, cancelling `cancel` along the way.
fn press_button(cancel: &'static str) -> Vec<String> {
    let mut harness = Harness::from_html_with(
        "<html><head><style>body { margin: 0 }</style></head><body>\
         <button id=b style='width: 100px; height: 40px'></button></body></html>",
        HarnessOptions {
            width: 200,
            height: 200,
            ..Default::default()
        },
    );

    let mut handler = CancellingHandler {
        cancel,
        seen: Vec::new(),
    };
    {
        let mut doc = harness.base_mut();
        let mut driver = EventDriver::new(&mut *doc, &mut handler);
        driver.handle_ui_event(UiEvent::PointerDown(press(50.0, 20.0)));
        driver.handle_ui_event(UiEvent::PointerUp(press(50.0, 20.0)));
    }
    handler.seen
}

#[test]
fn an_uncancelled_press_gives_the_mouse_events_and_the_click() {
    let seen = press_button("nothing");
    for expected in ["pointerdown", "mousedown", "pointerup", "mouseup", "click"] {
        assert!(
            seen.contains(&expected.to_string()),
            "expected {expected} in {seen:?}"
        );
    }
}

/// The case a gesture recognizer creates: it cancels the pointer events it
/// recognizes gestures from, and the buttons inside it still have to work.
#[test]
fn cancelling_pointer_up_keeps_the_click() {
    let seen = press_button("pointerup");
    assert!(
        !seen.contains(&"mouseup".to_string()),
        "the compatibility mouse event should be gone, got {seen:?}"
    );
    assert!(
        seen.contains(&"click".to_string()),
        "expected a click in {seen:?}"
    );
}

#[test]
fn cancelling_pointer_down_keeps_the_click() {
    let seen = press_button("pointerdown");
    assert!(
        !seen.contains(&"mousedown".to_string()),
        "the compatibility mouse event should be gone, got {seen:?}"
    );
    assert!(
        seen.contains(&"click".to_string()),
        "expected a click in {seen:?}"
    );
}
