//! Touch events (touchstart, touchmove, touchend) are generated from finger
//! pointer input and dispatched to application code alongside the corresponding
//! pointer events. Mouse input must NOT generate touch events (and vice-versa),
//! and default actions remain driven by the pointer events.

use blitz_dom::{Document, DocumentConfig, EventDriver, EventHandler};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{
        BlitzPointerEvent, BlitzPointerId, DomEvent, EventState, MouseEventButton,
        MouseEventButtons, Point, PointerCoords, PointerDetails, UiEvent,
    },
    shell::{ColorScheme, Viewport},
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

fn doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(200, 200, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn pointer_event(id: BlitzPointerId, x: f32, y: f32) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button: MouseEventButton::Main,
        buttons: MouseEventButtons::from(MouseEventButton::Main),
        mods: Default::default(),
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    }
}

/// An [`EventHandler`] that records the name of every [`DomEvent`] it sees.
#[derive(Clone, Default)]
struct RecordingHandler {
    events: Rc<RefCell<Vec<String>>>,
}

impl EventHandler for RecordingHandler {
    fn handle_event(
        &mut self,
        _chain: &[usize],
        event: &mut DomEvent,
        _doc: &mut dyn Document,
        _event_state: &mut EventState,
    ) {
        self.events.borrow_mut().push(event.name().to_string());
    }
}

fn drive(doc: &mut HtmlDocument, events: impl IntoIterator<Item = UiEvent>) -> Vec<String> {
    let handler = RecordingHandler::default();
    let recorded = handler.events.clone();
    let mut driver = EventDriver::new(doc, handler);
    for event in events {
        driver.handle_ui_event(event);
    }
    let recorded = recorded.borrow().clone();
    recorded
}

fn target_doc() -> HtmlDocument {
    doc(r#"<html><body style="margin:0">
        <div id="target" style="width:200px; height:200px;"></div>
    </body></html>"#)
}

#[test]
fn finger_input_generates_touch_events() {
    let mut doc = target_doc();
    let finger = BlitzPointerId::Finger(0);
    let names = drive(
        &mut doc,
        [
            UiEvent::PointerDown(pointer_event(finger, 50.0, 50.0)),
            UiEvent::PointerMove(pointer_event(finger, 60.0, 60.0)),
            UiEvent::PointerUp(pointer_event(finger, 60.0, 60.0)),
        ],
    );

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
    let mut doc = target_doc();
    let pen = BlitzPointerId::Pen;
    let names = drive(
        &mut doc,
        [
            UiEvent::PointerDown(pointer_event(pen, 50.0, 50.0)),
            UiEvent::PointerMove(pointer_event(pen, 60.0, 60.0)),
            UiEvent::PointerUp(pointer_event(pen, 60.0, 60.0)),
        ],
    );

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
    let mut doc = target_doc();
    let finger = BlitzPointerId::Finger(0);
    let names = drive(
        &mut doc,
        [
            UiEvent::PointerDown(pointer_event(finger, 50.0, 50.0)),
            UiEvent::PointerCancel(pointer_event(finger, 50.0, 50.0)),
        ],
    );

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
    let mut doc = target_doc();
    let pen = BlitzPointerId::Pen;
    let names = drive(
        &mut doc,
        [
            UiEvent::PointerDown(pointer_event(pen, 50.0, 50.0)),
            UiEvent::PointerCancel(pointer_event(pen, 50.0, 50.0)),
        ],
    );

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
    let mut doc = target_doc();
    let mouse = BlitzPointerId::Mouse;
    let names = drive(
        &mut doc,
        [
            UiEvent::PointerDown(pointer_event(mouse, 50.0, 50.0)),
            UiEvent::PointerCancel(pointer_event(mouse, 50.0, 50.0)),
        ],
    );

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
    let mut doc = target_doc();
    let mouse = BlitzPointerId::Mouse;
    let names = drive(
        &mut doc,
        [
            UiEvent::PointerDown(pointer_event(mouse, 50.0, 50.0)),
            UiEvent::PointerMove(pointer_event(mouse, 60.0, 60.0)),
            UiEvent::PointerUp(pointer_event(mouse, 60.0, 60.0)),
        ],
    );

    assert!(
        !names.iter().any(|n| n.starts_with("touch")),
        "mouse input should not generate touch events, got {names:?}"
    );
    // Mouse compatibility events are still generated.
    assert!(names.contains(&"mousedown".to_string()));
}
