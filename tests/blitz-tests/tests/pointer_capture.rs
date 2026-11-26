//! A captured pointer's events go to its capture target instead of the node under the
//! pointer, and the target stays hovered, until the pointer is lifted or cancelled.

use blitz_dom::{Document, EventDriver, EventHandler, NodeId};
use blitz_test_harness::{Harness, HarnessOptions, pointer_event};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, DomEvent, DomEventData, EventState, MouseEventButton,
    MouseEventButtons, UiEvent,
};
use keyboard_types::Modifiers;

const MOUSE: BlitzPointerId = BlitzPointerId::Mouse;

fn mouse_event(x: f32, y: f32) -> BlitzPointerEvent {
    pointer_event(
        MOUSE,
        x,
        y,
        MouseEventButton::Main,
        MouseEventButtons::from(MouseEventButton::Main),
        Modifiers::default(),
    )
}

/// Records the name and target of every event, and captures the pointer to `capture`
/// when it is pressed
struct CapturingHandler<'a> {
    capture: NodeId,
    events: &'a mut Vec<(&'static str, NodeId)>,
}

impl EventHandler for CapturingHandler<'_> {
    fn handle_event(
        &mut self,
        _chain: &[NodeId],
        event: &mut DomEvent,
        doc: &mut dyn Document,
        _event_state: &mut EventState,
    ) {
        self.events.push((event.name(), event.target));
        if let DomEventData::PointerDown(data) = &event.data {
            doc.inner_mut().set_pointer_capture(self.capture, data.id);
        }
    }
}

/// Dispatch the events with a [`CapturingHandler`], returning what it recorded
fn dispatch(
    harness: &mut Harness,
    capture: NodeId,
    events: impl IntoIterator<Item = UiEvent>,
) -> Vec<(&'static str, NodeId)> {
    let mut recorded = Vec::new();
    let mut doc = harness.base_mut();
    let handler = CapturingHandler {
        capture,
        events: &mut recorded,
    };
    let mut driver = EventDriver::new(&mut *doc, handler);
    for event in events {
        driver.handle_ui_event(event);
    }
    drop(doc);
    recorded
}

fn harness() -> Harness {
    Harness::from_html_with(
        r#"<html><body style="margin:0;display:flex">
            <div id="handle" style="width:100px;height:100px">handle</div>
            <div id="other" style="width:100px;height:100px">other</div>
        </body></html>"#,
        HarnessOptions {
            width: 200,
            height: 200,
            ..Default::default()
        },
    )
}

#[test]
fn captured_pointer_targets_and_hovers_the_capture_target() {
    let mut harness = harness();
    let handle = harness.node("#handle");
    let other = harness.node("#other");

    let events = dispatch(
        &mut harness,
        handle,
        [
            UiEvent::PointerDown(mouse_event(50.0, 50.0)),
            UiEvent::PointerMove(mouse_event(150.0, 50.0)),
            // The pointer keeps its capture when it is dragged out of the window
            UiEvent::PointerLeave(mouse_event(250.0, 50.0)),
            UiEvent::PointerMove(mouse_event(250.0, 50.0)),
        ],
    );

    assert!(events.contains(&("gotpointercapture", handle)));
    assert!(events.iter().all(|(_, target)| *target != other));
    assert_eq!(
        events
            .iter()
            .filter(|(name, _)| *name == "pointermove")
            .count(),
        2
    );
    assert_eq!(harness.hovered(), Some(handle));
    assert!(harness.base().has_pointer_capture(handle, MOUSE));
    // The drag does not select the text that it started on
    assert!(!harness.base().has_text_selection());

    let events = dispatch(
        &mut harness,
        handle,
        [UiEvent::PointerUp(mouse_event(150.0, 50.0))],
    );

    assert_eq!(events[0], ("pointerup", handle));
    assert!(events.contains(&("lostpointercapture", handle)));
    assert!(events.contains(&("pointerenter", other)));
    assert_eq!(harness.hovered(), Some(other));
    assert_eq!(harness.base().pointer_capture_target(MOUSE), None);
}

#[test]
fn cancelling_the_pointer_releases_its_capture() {
    let mut harness = harness();
    let handle = harness.node("#handle");

    let events = dispatch(
        &mut harness,
        handle,
        [
            UiEvent::PointerDown(mouse_event(50.0, 50.0)),
            UiEvent::PointerCancel(mouse_event(150.0, 50.0)),
        ],
    );

    assert!(events.contains(&("pointercancel", handle)));
    assert!(events.contains(&("lostpointercapture", handle)));
    assert_eq!(harness.base().pointer_capture_target(MOUSE), None);
}

#[test]
fn capturing_replaces_another_nodes_capture() {
    let mut harness = harness();
    let handle = harness.node("#handle");
    let other = harness.node("#other");

    let mut doc = harness.base_mut();
    assert!(doc.set_pointer_capture(handle, MOUSE));
    assert!(doc.set_pointer_capture(other, MOUSE));
    // Only the node that holds the capture can release it
    assert!(!doc.release_pointer_capture(MOUSE, Some(handle)));
    assert!(doc.has_pointer_capture(other, MOUSE));
    drop(doc);

    // The capture events are queued, and dispatched by the next event driver
    let events = dispatch(
        &mut harness,
        handle,
        [UiEvent::PointerMove(mouse_event(50.0, 150.0))],
    );
    assert_eq!(
        events[..3],
        [
            ("gotpointercapture", handle),
            ("lostpointercapture", handle),
            ("gotpointercapture", other),
        ]
    );
}

#[test]
fn removing_the_capture_target_releases_its_capture() {
    let mut harness = harness();
    let handle = harness.node("#handle");

    let mut doc = harness.base_mut();
    assert!(doc.set_pointer_capture(handle, MOUSE));
    doc.mutate().remove_and_drop_node(handle);
    assert_eq!(doc.pointer_capture_target(MOUSE), None);
}
