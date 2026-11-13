//! W3C Pointer Events Level 3 pointer capture conformance tests.

use blitz_dom::{
    Document, DocumentConfig, EventDriver, EventHandler, LocalName, NodeId, PlainDocument,
    PointerCaptureError, QualName, UserAgentStylesheet, ns,
};
use blitz_html::HtmlDocument;
use blitz_test_harness::pointer_event;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, DomEvent, DomEventData, EventState, MouseEventButton,
    MouseEventButtons, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use keyboard_types::Modifiers;
use std::cell::RefCell;
use std::rc::Rc;

fn make_doc(html: &str) -> PlainDocument {
    let config = DocumentConfig {
        viewport: Some(Viewport::new(300, 300, 1.0, ColorScheme::Light)),
        ua_stylesheets: vec![UserAgentStylesheet::default_sheet()],
        ..Default::default()
    };
    let mut doc = HtmlDocument::from_html(html, config).into_inner();
    doc.resolve(0.0);
    PlainDocument(doc)
}

fn node_id(doc: &PlainDocument, selector: &str) -> NodeId {
    doc.0.query_selector(selector).unwrap().expect(selector)
}

fn label_for(doc: &dyn Document, id: NodeId) -> String {
    let inner = doc.inner();
    if id == inner.root_node().id {
        return "#document".to_string();
    }
    let Some(node) = inner.get_node(id) else {
        return format!("missing({id})");
    };
    if let Some(el) = node.element_data() {
        if let Some(attr_id) = el.id.as_deref() {
            return format!("#{attr_id}");
        }
        return el.name.local.to_string();
    }
    format!("node({id})")
}

fn ptr_event(
    id: BlitzPointerId,
    x: f32,
    y: f32,
    button: MouseEventButton,
    buttons: MouseEventButtons,
) -> BlitzPointerEvent {
    pointer_event(id, x, y, button, buttons, Modifiers::default())
}

fn mouse(
    x: f32,
    y: f32,
    button: MouseEventButton,
    buttons: MouseEventButtons,
) -> BlitzPointerEvent {
    ptr_event(BlitzPointerId::Mouse, x, y, button, buttons)
}

fn touch(id: u64, x: f32, y: f32, buttons: MouseEventButtons) -> BlitzPointerEvent {
    ptr_event(
        BlitzPointerId::Finger(id),
        x,
        y,
        MouseEventButton::Main,
        buttons,
    )
}

struct HookHandler<F> {
    log: Rc<RefCell<Vec<String>>>,
    hook: F,
}

impl<F> HookHandler<F>
where
    F: FnMut(&mut DomEvent, &mut dyn Document, &mut EventState),
{
    fn new(log: Rc<RefCell<Vec<String>>>, hook: F) -> Self {
        Self { log, hook }
    }
}

impl<F> EventHandler for HookHandler<F>
where
    F: FnMut(&mut DomEvent, &mut dyn Document, &mut EventState),
{
    fn handle_event(
        &mut self,
        _chain: &[NodeId],
        event: &mut DomEvent,
        doc: &mut dyn Document,
        event_state: &mut EventState,
    ) {
        let target_label = label_for(doc, event.target);
        self.log
            .borrow_mut()
            .push(format!("{}@{target_label}", event.name()));
        (self.hook)(event, doc, event_state);
    }
}

fn send_with_hook<F>(
    doc: &mut PlainDocument,
    ui_event: UiEvent,
    log: &Rc<RefCell<Vec<String>>>,
    hook: F,
) where
    F: FnMut(&mut DomEvent, &mut dyn Document, &mut EventState),
{
    let handler = HookHandler::new(Rc::clone(log), hook);
    EventDriver::new(doc, handler).handle_ui_event(ui_event);
}

fn send(doc: &mut PlainDocument, ui_event: UiEvent, log: &Rc<RefCell<Vec<String>>>) {
    send_with_hook(doc, ui_event, log, |_, _, _| {});
}

#[test]
fn wpt_setpointercapture_invalid_pointerid() {
    let mut doc = make_doc(
        r#"<html><body style="margin:0"><div id="target" style="width:100px;height:100px"></div></body></html>"#,
    );
    let target = node_id(&doc, "#target");
    let log = Rc::new(RefCell::new(Vec::new()));

    for invalid_id in [-1, 0, 1, 999] {
        assert_eq!(
            doc.set_pointer_capture(target, invalid_id),
            Err(PointerCaptureError::NotFound)
        );
    }

    let touch_id = BlitzPointerId::Finger(0).to_dom_pointer_id();
    send(
        &mut doc,
        UiEvent::PointerDown(touch(0, 20.0, 20.0, MouseEventButtons::Primary)),
        &log,
    );
    send(
        &mut doc,
        UiEvent::PointerUp(touch(0, 20.0, 20.0, MouseEventButtons::None)),
        &log,
    );
    assert_eq!(
        doc.set_pointer_capture(target, touch_id),
        Err(PointerCaptureError::NotFound)
    );
}

#[test]
fn wpt_setpointercapture_disconnected() {
    let mut doc = make_doc(
        r#"<html><body style="margin:0"><div id="target" style="width:100px;height:100px"></div></body></html>"#,
    );
    let detached = {
        let mut mutator = doc.0.mutate();
        mutator.create_element(
            QualName::new(None, ns!(html), LocalName::from("div")),
            vec![],
        )
    };
    let log = Rc::new(RefCell::new(Vec::new()));

    // Unknown pointerId fails step 1 (NotFound) even when element is disconnected.
    assert_eq!(
        doc.set_pointer_capture(detached, 42),
        Err(PointerCaptureError::NotFound)
    );

    // Active pointerId on disconnected element fails step 2 (InvalidState).
    send(
        &mut doc,
        UiEvent::PointerDown(mouse(
            20.0,
            20.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    assert_eq!(
        doc.set_pointer_capture(detached, 1),
        Err(PointerCaptureError::InvalidState)
    );
}

#[test]
fn wpt_setpointercapture_inactive_button_mouse() {
    let mut doc = make_doc(
        r#"<html><body style="margin:0">
            <div id="target0" style="width:100px;height:100px"></div>
            <div id="capturer" style="width:100px;height:100px"></div>
        </body></html>"#,
    );
    let target0 = node_id(&doc, "#target0");
    let capturer = node_id(&doc, "#capturer");
    let log = Rc::new(RefCell::new(Vec::new()));

    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            20.0,
            20.0,
            MouseEventButton::Main,
            MouseEventButtons::None,
        )),
        &log,
    );
    log.borrow_mut().clear();

    assert_eq!(doc.set_pointer_capture(capturer, 1), Ok(()));
    assert!(!doc.has_pointer_capture(capturer, 1));

    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            30.0,
            30.0,
            MouseEventButton::Main,
            MouseEventButtons::None,
        )),
        &log,
    );
    assert_eq!(
        *log.borrow(),
        vec!["pointermove@#target0", "mousemove@#target0"]
    );
    assert_eq!(doc.0.get_hover_node_id(), Some(target0));
}

#[test]
fn wpt_releasepointercapture_invalid_id_and_non_owner_noop() {
    let mut doc = make_doc(
        r#"<html><body style="margin:0">
            <div id="a" style="width:100px;height:100px"></div>
            <div id="b" style="width:100px;height:100px"></div>
        </body></html>"#,
    );
    let a = node_id(&doc, "#a");
    let b = node_id(&doc, "#b");
    let log = Rc::new(RefCell::new(Vec::new()));

    assert_eq!(
        doc.release_pointer_capture(a, 77),
        Err(PointerCaptureError::NotFound)
    );

    send(
        &mut doc,
        UiEvent::PointerDown(mouse(
            20.0,
            20.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    assert_eq!(doc.set_pointer_capture(a, 1), Ok(()));
    assert!(doc.has_pointer_capture(a, 1));

    assert_eq!(doc.release_pointer_capture(b, 1), Ok(()));
    assert!(doc.has_pointer_capture(a, 1));

    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            20.0,
            150.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    assert!(log.borrow().contains(&"gotpointercapture@#a".to_string()));
    assert!(log.borrow().contains(&"pointermove@#a".to_string()));
}

#[test]
fn wpt_haspointercapture_tracks_pending_target() {
    let mut doc = make_doc(
        r#"<html><body style="margin:0">
            <div id="a" style="width:100px;height:100px"></div>
            <div id="b" style="width:100px;height:100px"></div>
        </body></html>"#,
    );
    let a = node_id(&doc, "#a");
    let b = node_id(&doc, "#b");
    let log = Rc::new(RefCell::new(Vec::new()));

    send(
        &mut doc,
        UiEvent::PointerDown(mouse(
            20.0,
            20.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );

    assert_eq!(doc.set_pointer_capture(a, 1), Ok(()));
    assert!(doc.has_pointer_capture(a, 1));
    assert!(!doc.has_pointer_capture(b, 1));

    assert_eq!(doc.set_pointer_capture(b, 1), Ok(()));
    assert!(!doc.has_pointer_capture(a, 1));
    assert!(doc.has_pointer_capture(b, 1));

    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            25.0,
            25.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    assert!(log.borrow().contains(&"gotpointercapture@#b".to_string()));
    assert!(!log.borrow().contains(&"gotpointercapture@#a".to_string()));

    assert_eq!(doc.release_pointer_capture(b, 1), Ok(()));
    assert!(!doc.has_pointer_capture(b, 1));

    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            25.0,
            25.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    assert_eq!(log.borrow()[0], "lostpointercapture@#b");
}

#[test]
fn wpt_boundary_events_in_capturing() {
    let mut doc = make_doc(
        r#"<html><body style="margin:0">
            <div id="target0" style="width:100px;height:100px"></div>
            <div id="capturer" style="width:100px;height:100px"></div>
        </body></html>"#,
    );
    let target0 = node_id(&doc, "#target0");
    let capturer = node_id(&doc, "#capturer");
    let log = Rc::new(RefCell::new(Vec::new()));

    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            20.0,
            20.0,
            MouseEventButton::Main,
            MouseEventButtons::None,
        )),
        &log,
    );
    send_with_hook(
        &mut doc,
        UiEvent::PointerDown(mouse(
            20.0,
            20.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
        |event, doc, _| {
            if event.name() == "pointerdown" {
                doc.set_pointer_capture(capturer, 1).unwrap();
            }
        },
    );
    log.borrow_mut().clear();

    // First move while still physically over #target0 transitions element_under_pointer to #capturer.
    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            25.0,
            25.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    let pointer_only: Vec<String> = log
        .borrow()
        .iter()
        .filter(|e| e.contains("pointer"))
        .cloned()
        .collect();
    assert_eq!(
        pointer_only,
        vec![
            "pointerout@#target0",
            "pointerleave@#target0",
            "pointerover@#capturer",
            "pointerenter@#capturer",
            "gotpointercapture@#capturer",
            "pointermove@#capturer",
        ]
    );
    assert!(doc.0.get_node(capturer).unwrap().is_hovered());
    assert!(!doc.0.get_node(target0).unwrap().is_hovered());

    // Subsequent moves during capture suppress hit-tested boundary events.
    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            30.0,
            30.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    let pointer_only: Vec<String> = log
        .borrow()
        .iter()
        .filter(|e| e.contains("pointer"))
        .cloned()
        .collect();
    assert_eq!(pointer_only, vec!["pointermove@#capturer"]);

    // Releasing over #target0 fires pointerup, lostpointercapture, click, then restores hover to #target0.
    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerUp(mouse(
            30.0,
            30.0,
            MouseEventButton::Main,
            MouseEventButtons::None,
        )),
        &log,
    );
    let relevant: Vec<String> = log
        .borrow()
        .iter()
        .filter(|e| e.contains("pointer") || e.starts_with("click@"))
        .cloned()
        .collect();
    assert_eq!(
        relevant,
        vec![
            "pointerup@#capturer",
            "lostpointercapture@#capturer",
            "click@#capturer",
            "pointerout@#capturer",
            "pointerleave@#capturer",
            "pointerover@#target0",
            "pointerenter@#target0",
        ]
    );
    assert!(doc.0.get_node(target0).unwrap().is_hovered());
    assert!(!doc.0.get_node(capturer).unwrap().is_hovered());
}

#[test]
fn wpt_click_during_capture_all_6_subcases_and_drag_suppression() {
    let html = r#"<html><body style="margin:0">
        <div id="container" style="width:300px;height:300px">
            <div id="blue" style="width:100px;height:60px">Selectable text content here</div>
            <div id="green" style="width:100px;height:60px"></div>
            <div id="grey" style="width:100px;height:60px"></div>
        </div>
    </body></html>"#;

    let run_case = |capture_on_down: Option<&str>,
                    release_during_drag: bool,
                    up_y: f32|
     -> (Vec<String>, bool) {
        let mut doc = make_doc(html);
        let capture_id = capture_on_down.map(|sel| node_id(&doc, sel));
        let log = Rc::new(RefCell::new(Vec::new()));

        send_with_hook(
            &mut doc,
            UiEvent::PointerDown(mouse(
                10.0,
                10.0,
                MouseEventButton::Main,
                MouseEventButtons::Primary,
            )),
            &log,
            |event, doc, _| {
                if event.name() == "pointerdown"
                    && let Some(target_id) = capture_id
                {
                    doc.set_pointer_capture(target_id, 1).unwrap();
                }
            },
        );

        send(
            &mut doc,
            UiEvent::PointerMove(mouse(
                80.0,
                10.0,
                MouseEventButton::Main,
                MouseEventButtons::Primary,
            )),
            &log,
        );

        if release_during_drag && let Some(target_id) = capture_id {
            doc.release_pointer_capture(target_id, 1).unwrap();
        }

        send(
            &mut doc,
            UiEvent::PointerMove(mouse(
                80.0,
                up_y,
                MouseEventButton::Main,
                MouseEventButtons::Primary,
            )),
            &log,
        );

        let had_selection = doc.0.has_text_selection();

        log.borrow_mut().clear();
        send(
            &mut doc,
            UiEvent::PointerUp(mouse(
                80.0,
                up_y,
                MouseEventButton::Main,
                MouseEventButtons::None,
            )),
            &log,
        );
        let events = log.borrow().clone();
        (events, had_selection)
    };

    // Subcase 1: no capture, down #blue (y=10), up #blue (y=10) -> click@#blue
    let (events, _) = run_case(None, false, 10.0);
    assert!(events.contains(&"click@#blue".to_string()), "{events:?}");

    // Subcase 2: no capture, down #blue (y=10), up #green (y=80) -> click@#container (LCA)
    let (events, _) = run_case(None, false, 80.0);
    assert!(
        events.contains(&"click@#container".to_string()),
        "{events:?}"
    );

    // Subcase 3: capture #grey, down #blue, up #blue -> lostpointercapture@#grey before click@#grey, no text selection
    let (events, had_selection) = run_case(Some("#grey"), false, 10.0);
    assert!(
        !had_selection,
        "explicit capture must suppress DragMode::Selecting"
    );
    let lost_idx = events
        .iter()
        .position(|e| e == "lostpointercapture@#grey")
        .expect("lostpointercapture@#grey");
    let click_idx = events
        .iter()
        .position(|e| e == "click@#grey")
        .expect("click@#grey");
    assert!(lost_idx < click_idx, "{events:?}");

    // Subcase 4: capture #grey, down #blue, up #green -> lostpointercapture@#grey before click@#grey
    let (events, _) = run_case(Some("#grey"), false, 80.0);
    let lost_idx = events
        .iter()
        .position(|e| e == "lostpointercapture@#grey")
        .expect("lostpointercapture@#grey");
    let click_idx = events
        .iter()
        .position(|e| e == "click@#grey")
        .expect("click@#grey");
    assert!(lost_idx < click_idx, "{events:?}");

    // Subcase 5: capture #green on down #blue, release capture during drag, up #green -> click@#container
    let (events, _) = run_case(Some("#green"), true, 80.0);
    assert!(
        events.contains(&"click@#container".to_string()),
        "{events:?}"
    );

    // Subcase 6: capture #green on down #blue, release capture during drag, up #blue -> click@#blue
    let (events, _) = run_case(Some("#green"), true, 10.0);
    assert!(events.contains(&"click@#blue".to_string()), "{events:?}");
}

#[test]
fn wpt_implicit_touch_capture_and_release_at_got_capture() {
    let mut doc = make_doc(
        r#"<html><body style="margin:0">
            <div id="target0" style="width:100px;height:100px;touch-action:none"></div>
            <div id="target1" style="width:100px;height:100px;touch-action:none"></div>
        </body></html>"#,
    );
    let target0 = node_id(&doc, "#target0");
    let touch_id = BlitzPointerId::Finger(0).to_dom_pointer_id();
    let log = Rc::new(RefCell::new(Vec::new()));

    // Part A: Implicit touch capture routes out-of-bounds drag and up/click to #target0.
    send(
        &mut doc,
        UiEvent::PointerDown(touch(0, 20.0, 20.0, MouseEventButtons::Primary)),
        &log,
    );
    assert!(doc.has_pointer_capture(target0, touch_id));

    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerMove(touch(0, 20.0, 150.0, MouseEventButtons::Primary)),
        &log,
    );
    assert_eq!(
        *log.borrow(),
        vec![
            "gotpointercapture@#target0",
            "pointermove@#target0",
            "touchmove@#target0",
        ]
    );

    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerUp(touch(0, 20.0, 150.0, MouseEventButtons::None)),
        &log,
    );
    assert_eq!(
        *log.borrow(),
        vec![
            "pointerup@#target0",
            "touchend@#target0",
            "lostpointercapture@#target0",
            "click@#target0",
            "pointerout@#target0",
            "pointerleave@html",
            "pointerleave@body",
            "pointerleave@#target0",
        ]
    );
    assert_eq!(doc.0.get_hover_node_id(), None);

    // Part B: Calling releasePointerCapture inside gotpointercapture still routes the
    // current pointermove to #target0, then fires lostpointercapture before the next move.
    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerDown(touch(0, 20.0, 20.0, MouseEventButtons::Primary)),
        &log,
    );
    log.borrow_mut().clear();

    send_with_hook(
        &mut doc,
        UiEvent::PointerMove(touch(0, 20.0, 150.0, MouseEventButtons::Primary)),
        &log,
        |event, doc, _| {
            if event.name() == "gotpointercapture" {
                doc.release_pointer_capture(target0, touch_id).unwrap();
                assert!(!doc.has_pointer_capture(target0, touch_id));
            }
        },
    );
    assert_eq!(
        *log.borrow(),
        vec![
            "gotpointercapture@#target0",
            "pointermove@#target0",
            "touchmove@#target0",
        ]
    );

    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerMove(touch(0, 20.0, 150.0, MouseEventButtons::Primary)),
        &log,
    );
    assert_eq!(
        *log.borrow(),
        vec![
            "lostpointercapture@#target0",
            "pointerout@#target0",
            "pointerleave@#target0",
            "pointerover@#target1",
            "pointerenter@#target1",
            "pointermove@#target1",
            "touchmove@#target1",
        ]
    );
}

#[test]
fn wpt_pointercapture_not_lost_in_chorded_buttons() {
    let mut doc = make_doc(
        r#"<html><body style="margin:0">
            <div id="target0" style="width:100px;height:100px"></div>
            <div id="target1" style="width:100px;height:100px"></div>
        </body></html>"#,
    );
    let target0 = node_id(&doc, "#target0");
    let log = Rc::new(RefCell::new(Vec::new()));
    let lost_buttons = Rc::new(RefCell::new(Vec::new()));

    send_with_hook(
        &mut doc,
        UiEvent::PointerDown(mouse(
            20.0,
            20.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
        |event, doc, _| {
            if event.name() == "pointerdown" {
                doc.set_pointer_capture(target0, 1).unwrap();
            }
        },
    );

    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            20.0,
            150.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    assert!(
        log.borrow()
            .contains(&"gotpointercapture@#target0".to_string())
    );
    log.borrow_mut().clear();

    // Press secondary button while primary is still held -> chorded pointermove + mousedown.
    send(
        &mut doc,
        UiEvent::PointerDown(mouse(
            20.0,
            150.0,
            MouseEventButton::Secondary,
            MouseEventButtons::Primary | MouseEventButtons::Secondary,
        )),
        &log,
    );
    assert_eq!(
        *log.borrow(),
        vec!["pointermove@#target0", "mousedown@#target0"]
    );
    assert!(doc.has_pointer_capture(target0, 1));

    // Release secondary button while primary remains held -> chorded pointermove + mouseup, capture retained.
    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerUp(mouse(
            20.0,
            150.0,
            MouseEventButton::Secondary,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    assert_eq!(
        *log.borrow(),
        vec!["pointermove@#target0", "mouseup@#target0"]
    );
    assert!(doc.has_pointer_capture(target0, 1));

    // Release final (primary) button -> pointerup, mouseup, lostpointercapture with buttons == 0.
    log.borrow_mut().clear();
    let lost_buttons_ref = Rc::clone(&lost_buttons);
    send_with_hook(
        &mut doc,
        UiEvent::PointerUp(mouse(
            20.0,
            150.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
        move |event, _, _| {
            if let DomEventData::LostPointerCapture(data) = &event.data {
                lost_buttons_ref.borrow_mut().push(data.buttons);
            }
        },
    );
    assert!(!doc.has_pointer_capture(target0, 1));
    assert!(log.borrow().contains(&"pointerup@#target0".to_string()));
    assert!(
        log.borrow()
            .contains(&"lostpointercapture@#target0".to_string())
    );
    assert_eq!(*lost_buttons.borrow(), vec![MouseEventButtons::None]);
}

#[test]
fn wpt_lostpointercapture_for_disconnected_node_and_slab_reuse() {
    let mut doc = make_doc(
        r#"<html><body style="margin:0">
            <div id="capturer" style="width:100px;height:100px"></div>
            <div id="other" style="width:100px;height:100px"></div>
        </body></html>"#,
    );
    let body = node_id(&doc, "body");
    let capturer = node_id(&doc, "#capturer");
    let log = Rc::new(RefCell::new(Vec::new()));

    send(
        &mut doc,
        UiEvent::PointerDown(mouse(
            20.0,
            20.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    doc.set_pointer_capture(capturer, 1).unwrap();
    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            25.0,
            25.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    assert!(
        log.borrow()
            .contains(&"gotpointercapture@#capturer".to_string())
    );

    // Remove #capturer and immediately allocate a new element reusing its underlying slot.
    let reused = {
        let mut mutator = doc.0.mutate();
        mutator.remove_and_drop_node(capturer);
        let new_el = mutator.create_element(
            QualName::new(None, ns!(html), LocalName::from("div")),
            vec![],
        );
        mutator.set_attribute(
            new_el,
            QualName::new(None, ns!(), LocalName::from("id")),
            "reused",
        );
        mutator.append_children(body, &[new_el]);
        new_el
    };
    doc.0.resolve(0.0);
    assert_eq!(reused.as_u64() as u32, capturer.as_u64() as u32);
    assert_ne!(reused, capturer);
    assert!(!doc.has_pointer_capture(reused, 1));

    log.borrow_mut().clear();
    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            20.0,
            50.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    assert_eq!(log.borrow()[0], "lostpointercapture@#document");
    assert!(log.borrow().contains(&"pointermove@#other".to_string()));
    assert!(!log.borrow().iter().any(|e| e.ends_with("@#reused")));
}

#[test]
fn wpt_remove_pending_capture_node_in_lost_and_got_handlers() {
    // Part A: Removing pending target #b inside #a's lostpointercapture prevents gotpointercapture@#b.
    let mut doc = make_doc(
        r#"<html><body style="margin:0">
            <div id="a" style="width:100px;height:80px"></div>
            <div id="b" style="width:100px;height:80px"></div>
            <div id="fallback" style="width:100px;height:80px"></div>
        </body></html>"#,
    );
    let a = node_id(&doc, "#a");
    let b = node_id(&doc, "#b");
    let log = Rc::new(RefCell::new(Vec::new()));

    send(
        &mut doc,
        UiEvent::PointerDown(mouse(
            20.0,
            20.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    doc.set_pointer_capture(a, 1).unwrap();
    send(
        &mut doc,
        UiEvent::PointerMove(mouse(
            25.0,
            25.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    doc.set_pointer_capture(b, 1).unwrap();
    log.borrow_mut().clear();

    send_with_hook(
        &mut doc,
        UiEvent::PointerMove(mouse(
            20.0,
            120.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
        move |event, doc, _| {
            if event.name() == "lostpointercapture" {
                let mut inner = doc.inner_mut();
                let mut mutator = inner.mutate();
                mutator.remove_and_drop_node(b);
                drop(mutator);
                inner.resolve(0.0);
            }
        },
    );
    assert_eq!(log.borrow()[0], "lostpointercapture@#a");
    assert!(
        !log.borrow()
            .iter()
            .any(|e| e.starts_with("gotpointercapture"))
    );
    assert!(log.borrow().contains(&"pointermove@#fallback".to_string()));

    // Part B: Removing captured target #b inside its own gotpointercapture immediately fires lostpointercapture@#document.
    let mut doc = make_doc(
        r#"<html><body style="margin:0">
            <div id="b" style="width:100px;height:80px"></div>
            <div id="fallback" style="width:100px;height:80px"></div>
        </body></html>"#,
    );
    let body = node_id(&doc, "body");
    let b = node_id(&doc, "#b");
    let log = Rc::new(RefCell::new(Vec::new()));

    send(
        &mut doc,
        UiEvent::PointerDown(mouse(
            20.0,
            20.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
    );
    doc.set_pointer_capture(b, 1).unwrap();
    log.borrow_mut().clear();

    send_with_hook(
        &mut doc,
        UiEvent::PointerMove(mouse(
            20.0,
            40.0,
            MouseEventButton::Main,
            MouseEventButtons::Primary,
        )),
        &log,
        move |event, doc, _| {
            if event.name() == "gotpointercapture" {
                let mut inner = doc.inner_mut();
                let mut mutator = inner.mutate();
                mutator.remove_and_drop_node(b);
                let reused = mutator.create_element(
                    QualName::new(None, ns!(html), LocalName::from("span")),
                    vec![],
                );
                mutator.append_children(body, &[reused]);
                drop(mutator);
                inner.resolve(0.0);
            }
        },
    );
    let got_idx = log
        .borrow()
        .iter()
        .position(|e| e == "gotpointercapture@#b")
        .expect("gotpointercapture@#b");
    let lost_idx = log
        .borrow()
        .iter()
        .position(|e| e == "lostpointercapture@#document")
        .expect("lostpointercapture@#document");
    let move_idx = log
        .borrow()
        .iter()
        .position(|e| e == "pointermove@#fallback")
        .expect("pointermove@#fallback");
    assert!(
        got_idx < lost_idx && lost_idx < move_idx,
        "{:?}",
        *log.borrow()
    );
}
