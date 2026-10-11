//! A pointer leaving the document (e.g. moving out of the window) ends hover and
//! fires out/leave events, and a sub-document's hover ends when the pointer
//! leaves its host.

use blitz_dom::{
    BaseDocument, DEFAULT_CSS, DocumentConfig, EventDriver, LocalName, NodeId, NoopEventHandler,
    PlainDocument, QualName, ns,
};
use blitz_html::HtmlDocument;
use blitz_test_harness::{Harness, HarnessOptions, pointer_event};
use blitz_traits::events::{BlitzPointerId, MouseEventButton, MouseEventButtons, UiEvent};
use blitz_traits::shell::ShellProvider;
use cursor_icon::CursorIcon;
use keyboard_types::Modifiers;
use std::sync::{Arc, Mutex};

fn mouse_event(x: f32, y: f32) -> blitz_traits::events::BlitzPointerEvent {
    pointer_event(
        BlitzPointerId::Mouse,
        x,
        y,
        MouseEventButton::Main,
        MouseEventButtons::empty(),
        Modifiers::default(),
    )
}

#[test]
fn leaving_document_clears_hover_and_fires_leave_events() {
    let mut harness = Harness::from_html_with(
        r#"<html><body style="margin:0"><div id="target" style="width:100px;height:100px"></div></body></html>"#,
        HarnessOptions {
            width: 200,
            height: 200,
            ..Default::default()
        },
    );
    let target = harness.node("#target");
    harness.move_mouse_to(50.0, 50.0);
    assert_eq!(harness.hovered(), Some(target));

    let events = harness.dispatch_recorded([UiEvent::PointerLeave(mouse_event(50.0, -1.0))]);
    harness.pump();

    assert_eq!(
        &events[..4],
        ["pointerout", "mouseout", "pointerleave", "mouseleave"]
    );
    assert!(!events.iter().any(|name| name == "pointermove"));
    assert_eq!(harness.hovered(), None);
    assert!(!harness.base().get_node(target).unwrap().is_hovered());
}

/// Records the cursor requests a window would receive
#[derive(Default)]
struct Window {
    cursors: Mutex<Vec<Option<CursorIcon>>>,
}

impl ShellProvider for Window {
    fn set_cursor(&self, icon: Option<CursorIcon>) {
        self.cursors.lock().unwrap().push(icon);
    }
}

const PARENT: &str = r#"<html><body style="margin:0">
<div id="host" style="width:300px;height:200px"></div>
<div id="outside" style="width:300px;height:100px;cursor:pointer"></div>
</body></html>"#;

const CHILD: &str = r#"<html><head><style>
body{margin:0}
#go{display:block;width:200px;height:50px}
</style></head><body><a id="go" href="next.html">link</a></body></html>"#;

fn document(html: &str, window: &Arc<Window>) -> BaseDocument {
    let config = DocumentConfig {
        shell_provider: Some(window.clone()),
        ua_stylesheets: Some(vec![DEFAULT_CSS.to_string()]),
        ..Default::default()
    };
    HtmlDocument::from_html(html, config).into_inner()
}

fn hosted_doc(window: &Arc<Window>) -> (PlainDocument, NodeId) {
    let mut parent = document(PARENT, window);
    parent.viewport_mut().window_size = (300, 300);
    let host = parent.query_selector("#host").unwrap().unwrap();
    parent.set_sub_document(host, Box::new(PlainDocument(document(CHILD, window))));
    parent.resolve(0.0);
    (PlainDocument(parent), host)
}

fn send(parent: &mut PlainDocument, event: UiEvent) {
    EventDriver::new(parent, NoopEventHandler).handle_ui_event(event);
    parent.0.resolve(0.0);
}

fn child_hover(parent: &PlainDocument, host: NodeId) -> Option<NodeId> {
    parent.0.subdoc(host).unwrap().inner().get_hover_node_id()
}

fn child_go(parent: &PlainDocument, host: NodeId) -> NodeId {
    let child = parent.0.subdoc(host).unwrap().inner();
    child.query_selector("#go").unwrap().unwrap()
}

#[test]
fn leaving_host_clears_sub_document_hover() {
    let window = Arc::new(Window::default());
    let (mut parent, host) = hosted_doc(&window);
    let go = child_go(&parent, host);

    send(&mut parent, UiEvent::PointerMove(mouse_event(100.0, 25.0)));
    assert_eq!(child_hover(&parent, host), Some(go));

    send(&mut parent, UiEvent::PointerMove(mouse_event(100.0, 250.0)));
    assert_eq!(child_hover(&parent, host), None);
    let child = parent.0.subdoc(host).unwrap().inner();
    assert!(!child.get_node(go).unwrap().is_hovered());
}

#[test]
fn sub_document_does_not_rehover_after_pointer_left_host() {
    let window = Arc::new(Window::default());
    let (mut parent, host) = hosted_doc(&window);
    let go = child_go(&parent, host);

    send(&mut parent, UiEvent::PointerMove(mouse_event(100.0, 25.0)));
    send(&mut parent, UiEvent::PointerMove(mouse_event(100.0, 250.0)));
    let cursor_requests = window.cursors.lock().unwrap().len();

    // Changing the child's layout must not make it re-resolve hover at its
    // last pointer position (and request a cursor) while the pointer is
    // outside the host.
    {
        let mut child = parent.0.subdoc_mut(host).unwrap().inner_mut();
        let mut mutator = child.mutate();
        mutator.set_attribute(
            go,
            QualName::new(None, ns!(), LocalName::from("style")),
            "display:none",
        );
    }
    parent.0.resolve(0.0);
    parent.0.resolve(0.0);

    assert_eq!(child_hover(&parent, host), None);
    assert_eq!(window.cursors.lock().unwrap().len(), cursor_requests);
}

#[test]
fn leaving_window_clears_sub_document_hover() {
    let window = Arc::new(Window::default());
    let (mut parent, host) = hosted_doc(&window);
    let go = child_go(&parent, host);

    send(&mut parent, UiEvent::PointerMove(mouse_event(100.0, 25.0)));
    assert_eq!(child_hover(&parent, host), Some(go));

    send(&mut parent, UiEvent::PointerLeave(mouse_event(100.0, -1.0)));
    assert_eq!(parent.0.get_hover_node_id(), None);
    assert_eq!(child_hover(&parent, host), None);
}

fn finger_event(x: f32, y: f32) -> blitz_traits::events::BlitzPointerEvent {
    pointer_event(
        BlitzPointerId::Finger(0),
        x,
        y,
        MouseEventButton::Main,
        MouseEventButtons::from(MouseEventButton::Main),
        Modifiers::default(),
    )
}

fn assert_finger_ends_hover(end: UiEvent, end_name: &str) {
    let mut harness = Harness::from_html_with(
        r#"<html><body style="margin:0"><div id="target" style="width:100px;height:100px"></div></body></html>"#,
        HarnessOptions {
            width: 200,
            height: 200,
            ..Default::default()
        },
    );
    let target = harness.node("#target");
    let events = harness.dispatch_recorded([UiEvent::PointerDown(finger_event(50.0, 50.0)), end]);
    harness.pump();

    // A finger can't hover, so lifting or cancelling it ends hover with out/leave events
    let end_index = events.iter().position(|name| name == end_name).unwrap();
    let mut after: Vec<_> = events[end_index..]
        .iter()
        .filter(|name| name.starts_with("pointer"))
        .collect();
    // One pointerleave per element in the hovered chain
    after.dedup();
    assert_eq!(
        after,
        [end_name, "pointerout", "pointerleave"],
        "{events:?}"
    );
    assert_eq!(harness.hovered(), None);
    assert!(!harness.base().get_node(target).unwrap().is_hovered());
}

#[test]
fn finger_up_ends_hover_with_leave_events() {
    assert_finger_ends_hover(UiEvent::PointerUp(finger_event(50.0, 50.0)), "pointerup");
}

#[test]
fn finger_cancel_ends_hover_with_leave_events() {
    assert_finger_ends_hover(
        UiEvent::PointerCancel(finger_event(50.0, 50.0)),
        "pointercancel",
    );
}

#[test]
fn cursor_is_set_after_sub_document_handles_event() {
    let window = Arc::new(Window::default());
    let (mut parent, _host) = hosted_doc(&window);
    let cursors = || window.cursors.lock().unwrap().clone();

    // Entering the child's link must not hide the cursor first because the
    // child has not seen the move yet when the parent's hover changes
    send(&mut parent, UiEvent::PointerMove(mouse_event(100.0, 25.0)));
    assert!(!cursors().contains(&None), "{:?}", cursors());
    assert_eq!(cursors().last(), Some(&Some(CursorIcon::Pointer)));

    // Leaving the host for a parent element must not let the child (which
    // has lost its hover) hide the cursor
    send(&mut parent, UiEvent::PointerMove(mouse_event(100.0, 250.0)));
    assert!(!cursors().contains(&None), "{:?}", cursors());
    assert_eq!(cursors().last(), Some(&Some(CursorIcon::Pointer)));
}
