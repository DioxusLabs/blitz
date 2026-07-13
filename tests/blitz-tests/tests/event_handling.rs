//! End-to-end tests for Dioxus event handling implemented on top of blitz-dom's
//! event listener registry: vdom event handlers are registered as delegated (bubbling)
//! or per-element (non-bubbling) listeners, and dispatched events are routed to the
//! vdom via the `data-dioxus-id` attribute of the nearest managed element.

use blitz_dom::Document as _;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus::prelude::*;
use dioxus_native_dom::{DioxusDocument, DocumentConfig};
use std::sync::atomic::{AtomicUsize, Ordering};

fn pointer_event(x: f32, y: f32) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
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

fn make_doc(app: fn() -> Element) -> DioxusDocument {
    let vdom = VirtualDom::new(app);
    let mut doc = DioxusDocument::new(
        vdom,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    );
    doc.initial_build();
    doc.inner.borrow_mut().resolve(0.0);
    doc
}

fn click_at(doc: &mut DioxusDocument, x: f32, y: f32) {
    doc.handle_ui_event(UiEvent::PointerDown(pointer_event(x, y)));
    doc.handle_ui_event(UiEvent::PointerUp(pointer_event(x, y)));
}

#[test]
fn click_events_are_delegated_to_vdom_handlers() {
    static CLICKS: AtomicUsize = AtomicUsize::new(0);

    fn app() -> Element {
        rsx! {
            div {
                style: "width: 100px; height: 100px;",
                onclick: move |_| {
                    CLICKS.fetch_add(1, Ordering::SeqCst);
                },
                div { style: "width: 50px; height: 50px;" }
            }
        }
    }

    // The click lands on the inner child div, which has no handler of its own:
    // the delegated listener must route the event to the parent's onclick handler
    let mut doc = make_doc(app);
    click_at(&mut doc, 25.0, 25.0);
    assert_eq!(CLICKS.load(Ordering::SeqCst), 1);

    // A click outside of the app's elements must not trigger the handler
    click_at(&mut doc, 500.0, 500.0);
    assert_eq!(CLICKS.load(Ordering::SeqCst), 1);
}

#[test]
fn non_bubbling_events_reach_vdom_handlers() {
    static FOCUSSES: AtomicUsize = AtomicUsize::new(0);

    fn app() -> Element {
        rsx! {
            input {
                r#type: "text",
                style: "width: 200px; height: 40px;",
                onfocus: move |_| {
                    FOCUSSES.fetch_add(1, Ordering::SeqCst);
                },
            }
        }
    }

    // Clicking the input focusses it, which dispatches a (non-bubbling) focus event
    // to a listener registered on the input element itself
    let mut doc = make_doc(app);
    click_at(&mut doc, 30.0, 20.0);
    assert_eq!(FOCUSSES.load(Ordering::SeqCst), 1);
}

#[test]
fn prevent_default_from_vdom_handler_cancels_default_action() {
    fn checked_app() -> Element {
        rsx! {
            input { r#type: "checkbox", style: "width: 20px; height: 20px;" }
        }
    }

    fn prevented_app() -> Element {
        rsx! {
            input {
                r#type: "checkbox",
                style: "width: 20px; height: 20px;",
                onclick: move |event| event.prevent_default(),
            }
        }
    }

    let checkbox_checked = |doc: &DioxusDocument| {
        let inner = doc.inner.borrow();
        let input_id = inner.query_selector("input").unwrap().unwrap();
        inner
            .get_node(input_id)
            .unwrap()
            .element_data()
            .unwrap()
            .checkbox_input_checked()
            .unwrap()
    };

    // Without prevent_default, clicking a checkbox toggles it (the default action)
    let mut doc = make_doc(checked_app);
    click_at(&mut doc, 15.0, 15.0);
    assert!(checkbox_checked(&doc));

    // A vdom handler which calls prevent_default cancels the toggle
    let mut doc = make_doc(prevented_app);
    click_at(&mut doc, 15.0, 15.0);
    assert!(!checkbox_checked(&doc));
}
