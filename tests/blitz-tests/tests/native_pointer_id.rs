//! Dioxus Native gives mouse, pen and each finger distinct pointer IDs.

use blitz_test_harness::{Harness, pointer_event};
use blitz_traits::events::{BlitzPointerId, MouseEventButton, MouseEventButtons, UiEvent};
use dioxus::prelude::*;

fn app() -> Element {
    let mut seen = use_signal(Vec::<String>::new);
    rsx! {
        div {
            id: "target",
            style: "width: 100px; height: 100px",
            onpointerdown: move |event| {
                seen.write().push(format!("{} {}", event.pointer_type(), event.pointer_id()));
            },
        }
        div { id: "seen", "{seen.read().join(\", \")}" }
    }
}

#[test]
fn mouse_pen_and_fingers_get_distinct_pointer_ids() {
    let mut harness = Harness::from_component(app);
    for id in [
        BlitzPointerId::Mouse,
        BlitzPointerId::Pen,
        BlitzPointerId::finger(0),
        BlitzPointerId::finger(1),
    ] {
        let event = pointer_event(
            id,
            50.0,
            50.0,
            MouseEventButton::Main,
            MouseEventButtons::from(MouseEventButton::Main),
            Default::default(),
        );
        harness.dispatch(UiEvent::PointerDown(event));
        harness.pump();
    }
    assert_eq!(
        harness.text_content("#seen"),
        "mouse 0, pen 1, touch 2, touch 3"
    );
}
