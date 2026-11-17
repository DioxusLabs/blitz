use blitz_dom::{Document, EventDriver, EventHandler};
use blitz_test_harness::{Harness, mouse_pointer_event};
use blitz_traits::events::{DomEvent, DomEventData, DomEventKind, EventState, UiEvent};
use blitz_traits::node_id::NodeId;
use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecordedFormEvent {
    name: &'static str,
    target: NodeId,
    bubbles: bool,
    cancelable: bool,
    value: String,
    bubbled_to: Vec<NodeId>,
}

fn dispatch_form_events(
    harness: &mut Harness,
    events: impl IntoIterator<Item = UiEvent>,
) -> Vec<RecordedFormEvent> {
    #[derive(Clone, Default)]
    struct Handler(Rc<RefCell<Vec<RecordedFormEvent>>>);

    impl EventHandler for Handler {
        fn handle_event(
            &mut self,
            chain: &[NodeId],
            event: &mut DomEvent,
            _doc: &mut dyn Document,
            _event_state: &mut EventState,
        ) {
            if let DomEventData::Input(data) | DomEventData::Change(data) = &event.data {
                self.0.borrow_mut().push(RecordedFormEvent {
                    name: event.name(),
                    target: event.target,
                    bubbles: event.bubbles,
                    cancelable: event.cancelable,
                    value: data.value.clone(),
                    bubbled_to: chain.to_vec(),
                });
            }
        }
    }

    let out = Rc::new(RefCell::new(Vec::new()));
    {
        let mut doc = harness.base_mut();
        let mut driver = EventDriver::new(&mut *doc, Handler(Rc::clone(&out)));
        for event in events {
            driver.handle_ui_event(event);
        }
    }
    harness.pump();
    Rc::try_unwrap(out).unwrap().into_inner()
}

fn click_form_events(harness: &mut Harness, selector: &str) -> Vec<RecordedFormEvent> {
    let (x, y) = harness.center_of(selector);
    let ptr = mouse_pointer_event(x, y);
    dispatch_form_events(
        harness,
        [UiEvent::PointerDown(ptr.clone()), UiEvent::PointerUp(ptr)],
    )
}

#[test]
fn checkbox_toggle_dispatches_input_then_change() {
    assert_eq!(DomEventKind::from_str("change"), Ok(DomEventKind::Change));
    assert_eq!(DomEventKind::from_str("onchange"), Ok(DomEventKind::Change));

    let mut harness = Harness::from_html(
        r#"<html><body><div id="wrap"><input id="cb" type="checkbox" style="width:20px;height:20px"></div></body></html>"#,
    );
    let wrap = harness.node("#wrap");
    let cb = harness.node("#cb");

    let first = click_form_events(&mut harness, "#cb");
    assert_eq!(first.len(), 2);
    assert_eq!(
        (&first[0].name, first[0].target, &first[0].value[..]),
        (&"input", cb, "true")
    );
    assert_eq!(
        (
            &first[1].name,
            first[1].target,
            first[1].bubbles,
            first[1].cancelable,
            &first[1].value[..]
        ),
        (&"change", cb, true, false, "true")
    );
    assert!(first[1].bubbled_to.contains(&wrap));

    let second = click_form_events(&mut harness, "#cb");
    assert_eq!(
        second
            .iter()
            .map(|e| (e.name, e.value.as_str()))
            .collect::<Vec<_>>(),
        [("input", "false"), ("change", "false")]
    );
}

#[test]
fn radio_click_dispatches_input_and_change_only_when_newly_checked() {
    let mut harness = Harness::from_html(
        r#"<html><body>
            <input id="r1" type="radio" name="g" style="width:20px;height:20px">
            <input id="r2" type="radio" name="g" style="width:20px;height:20px">
        </body></html>"#,
    );
    let r1 = harness.node("#r1");
    let r2 = harness.node("#r2");

    let first = click_form_events(&mut harness, "#r1");
    assert_eq!(
        first
            .iter()
            .map(|e| (e.name, e.target, e.value.as_str()))
            .collect::<Vec<_>>(),
        [("input", r1, "true"), ("change", r1, "true")]
    );

    assert!(click_form_events(&mut harness, "#r1").is_empty());

    let switch = click_form_events(&mut harness, "#r2");
    assert_eq!(
        switch
            .iter()
            .map(|e| (e.name, e.target, e.value.as_str()))
            .collect::<Vec<_>>(),
        [("input", r2, "true"), ("change", r2, "true")]
    );
}
