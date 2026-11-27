//! A Dioxus component that captures the pointer on pointerdown keeps getting its moves
//! when the pointer is dragged off it.

use std::rc::Rc;

use blitz_test_harness::{Harness, HarnessOptions, mouse_pointer_event};
use blitz_traits::events::UiEvent;
use dioxus_core::prelude::*;
use dioxus_vdom::VirtualDom;

fn app() -> Element {
    let mut mounted = use_signal(|| None::<Rc<MountedData>>);
    let mut seen = use_signal(Vec::<String>::new);
    rsx! {
        div { style: "display: flex",
            div {
                id: "handle",
                style: "width: 100px; height: 100px",
                onmounted: move |event| mounted.set(Some(event.data())),
                onpointerdown: move |event| {
                    let Some(mounted) = mounted() else { return };
                    let capture = mounted.set_pointer_capture(event.pointer_id());
                    spawn(async move {
                        if let Err(err) = capture.await {
                            seen.write().push(err.to_string());
                        }
                    });
                },
                onpointermove: move |_| seen.write().push("handle".into()),
            }
            div {
                id: "other",
                style: "width: 100px; height: 100px",
                onpointermove: move |_| seen.write().push("other".into()),
            }
        }
        div { id: "seen", "{seen.read().join(\", \")}" }
    }
}

#[test]
fn captured_pointer_moves_go_to_the_capturing_component() {
    let mut harness = Harness::from_vdom(
        VirtualDom::new(app),
        HarnessOptions {
            width: 200,
            height: 200,
            ..Default::default()
        },
    );

    harness.drag((50.0, 50.0), (150.0, 50.0), 1);
    harness.touch_down(0, 50.0, 50.0);
    harness.touch_move(0, 150.0, 50.0);
    harness.touch_up(0, 150.0, 50.0);
    assert_eq!(harness.text_content("#seen"), "handle, handle");

    // Without a capture the moves follow the pointer
    harness.move_mouse_to(150.0, 50.0);
    assert_eq!(harness.text_content("#seen"), "handle, handle, other");
}

fn lifecycle_app() -> Element {
    let mut handle = use_signal(|| None::<Rc<MountedData>>);
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        div { style: "display: flex",
            div {
                id: "handle",
                style: "width: 100px; height: 100px",
                onmounted: move |event| handle.set(Some(event.data())),
                onpointerdown: move |event| {
                    let element = handle().unwrap();
                    spawn(async move {
                        let result = element.set_pointer_capture(event.pointer_id()).await;
                        log.push(format!("capture {}", result.is_ok()));
                    });
                },
                ongotpointercapture: move |_| log.push("got".into()),
                onlostpointercapture: move |_| log.push("lost".into()),
                onpointermove: move |_| log.push("handle move".into()),
                onpointerup: move |event| {
                    let element = handle().unwrap();
                    spawn(async move {
                        let result = element.release_pointer_capture(event.pointer_id()).await;
                        log.push(format!("release {}", result.is_ok()));
                    });
                },
            }
            div {
                id: "other",
                style: "width: 100px; height: 100px",
                onpointermove: move |_| log.push("other move".into()),
            }
        }
        div { id: "log", {log().join(", ")} }
    }
}

#[test]
fn captured_pointer_events_go_to_the_capturing_element() {
    let mut harness = Harness::from_component(lifecycle_app);

    harness.mouse_down_at(50.0, 50.0);
    harness.dispatch(UiEvent::PointerMove(mouse_pointer_event(150.0, 50.0)));
    harness.pump();
    // The pointer leaves the window while captured
    harness.dispatch(UiEvent::PointerLeave(mouse_pointer_event(150.0, 50.0)));
    harness.pump();
    harness.dispatch(UiEvent::PointerMove(mouse_pointer_event(900.0, -40.0)));
    harness.pump();
    harness.mouse_up_at(150.0, 50.0);
    harness.move_mouse_to(150.0, 60.0);

    assert_eq!(
        harness.text_content("#log"),
        "capture true, got, handle move, handle move, lost, release true, other move"
    );
}
