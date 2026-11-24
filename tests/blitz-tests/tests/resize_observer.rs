//! Resize observer notifications in `blitz-dom` and `onresize` in `dioxus-native-dom`.

use blitz_dom::{Document, EventDriver, EventHandler, NodeId};
use blitz_test_harness::Harness;
use blitz_traits::events::{BlitzResizeEvent, DomEvent, DomEventData, EventState};
use dioxus::prelude::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
struct ResizeRecorder(Rc<RefCell<Vec<(NodeId, BlitzResizeEvent)>>>);

impl EventHandler for ResizeRecorder {
    fn handle_event(
        &mut self,
        _chain: &[NodeId],
        event: &mut DomEvent,
        _doc: &mut dyn Document,
        _event_state: &mut EventState,
    ) {
        if let DomEventData::Resize(data) = event.data {
            self.0.borrow_mut().push((event.target, data));
        }
    }
}

fn drain_resizes(doc: &mut blitz_dom::BaseDocument) -> Vec<(NodeId, BlitzResizeEvent)> {
    let recorder = ResizeRecorder::default();
    EventDriver::new(doc, recorder.clone()).flush_pending_events();
    recorder.0.borrow().clone()
}

#[test]
fn observe_resize_reports_initial_and_changed_sizes_with_refcounting() {
    let html = r#"
        <html><body style="margin: 0">
            <div id="box" style="width: 50%; height: 100px; padding: 10px; border: 5px solid black; box-sizing: border-box;"></div>
        </body></html>
    "#;
    let mut harness = Harness::from_html(html);
    let box_id = harness.base().query_selector("#box").unwrap().unwrap();

    // Observe twice to exercise refcounting
    {
        let mut doc = harness.base_mut();
        doc.observe_resize(box_id);
        doc.observe_resize(box_id);
        doc.resolve(0.0);
        let events = drain_resizes(&mut doc);
        assert_eq!(
            events,
            vec![(
                box_id,
                BlitzResizeEvent {
                    border_box_width: 400.0,
                    border_box_height: 100.0,
                    content_box_width: 370.0,
                    content_box_height: 70.0,
                }
            )]
        );

        // Re-resolving without a size change queues nothing
        doc.resolve(0.0);
        assert!(!doc.has_pending_events());
    }

    // One unobserve leaves refcount at 1, so viewport resize still fires
    harness.base_mut().unobserve_resize(box_id);
    harness.set_viewport_size(600, 400);
    {
        let mut doc = harness.base_mut();
        let events = drain_resizes(&mut doc);
        assert_eq!(
            events,
            vec![(
                box_id,
                BlitzResizeEvent {
                    border_box_width: 300.0,
                    border_box_height: 100.0,
                    content_box_width: 270.0,
                    content_box_height: 70.0,
                }
            )]
        );
    }

    // Second unobserve drops refcount to 0, so further resizes fire nothing
    harness.base_mut().unobserve_resize(box_id);
    harness.set_viewport_size(500, 400);
    assert!(!harness.base().has_pending_events());
}

#[test]
fn observe_resize_handles_display_none_and_node_removal() {
    let html = r#"
        <html><body style="margin: 0">
            <div id="box" style="width: 120px; height: 80px;"></div>
        </body></html>
    "#;
    let mut harness = Harness::from_html(html);
    let box_id = harness.base().query_selector("#box").unwrap().unwrap();

    {
        let mut doc = harness.base_mut();
        doc.observe_resize(box_id);
        doc.resolve(0.0);
        let events = drain_resizes(&mut doc);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.border_box_width, 120.0);
        assert_eq!(events[0].1.border_box_height, 80.0);

        // Hiding with display: none reports 0x0
        doc.mutate().set_attribute(
            box_id,
            markup5ever::QualName::new(None, markup5ever::ns!(), markup5ever::local_name!("style")),
            "display: none;",
        );
        doc.resolve(0.0);
        let events = drain_resizes(&mut doc);
        assert_eq!(
            events,
            vec![(
                box_id,
                BlitzResizeEvent {
                    border_box_width: 0.0,
                    border_box_height: 0.0,
                    content_box_width: 0.0,
                    content_box_height: 0.0,
                }
            )]
        );

        // Removing the node cleans up its observer entry
        doc.mutate().remove_and_drop_node(box_id);
        doc.resolve(0.0);
        assert!(!doc.has_pending_events());
    }
}

fn dioxus_resize_app() -> Element {
    let mut reported = use_signal(String::new);
    rsx! {
        style { "body {{ margin: 0; }}" }
        div {
            id: "target",
            style: "width: 50%; height: 100px; padding: 10px; box-sizing: border-box;",
            onresize: move |event| {
                let border = event.data().get_border_box_size().unwrap();
                let content = event.data().get_content_box_size().unwrap();
                reported.set(format!(
                    "{}x{} / {}x{}",
                    border.width, border.height, content.width, content.height
                ));
            },
        }
        div { id: "reported", "{reported}" }
    }
}

#[test]
fn dioxus_onresize_delivers_border_and_content_box_sizes_same_frame() {
    let mut harness = Harness::from_component(dioxus_resize_app);
    assert_eq!(harness.text_content("#reported"), "400x100 / 380x80");

    harness.set_viewport_size(600, 400);
    assert_eq!(harness.text_content("#reported"), "300x100 / 280x80");
}
