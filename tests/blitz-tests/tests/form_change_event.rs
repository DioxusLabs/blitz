use blitz_dom::{Document, EventDriver, EventHandler};
use blitz_test_harness::{Harness, key_event, mouse_pointer_event};
use blitz_traits::events::{DomEvent, DomEventData, DomEventKind, EventState, KeyState, UiEvent};
use blitz_traits::node_id::NodeId;
use keyboard_types::{Key, Modifiers, NamedKey};
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

fn press_key(harness: &mut Harness, key: Key) -> Vec<RecordedFormEvent> {
    dispatch_form_events(
        harness,
        [
            UiEvent::KeyDown(key_event(
                key.clone(),
                KeyState::Pressed,
                Modifiers::empty(),
            )),
            UiEvent::KeyUp(key_event(key, KeyState::Released, Modifiers::empty())),
        ],
    )
}

fn type_chars(harness: &mut Harness, text: &str) -> Vec<RecordedFormEvent> {
    let events = text.chars().flat_map(|ch| {
        let key = Key::Character(ch.to_string());
        [
            UiEvent::KeyDown(key_event(
                key.clone(),
                KeyState::Pressed,
                Modifiers::empty(),
            )),
            UiEvent::KeyUp(key_event(key, KeyState::Released, Modifiers::empty())),
        ]
    });
    dispatch_form_events(harness, events)
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

#[test]
fn text_input_and_textarea_commit_change_on_blur() {
    let mut harness = Harness::from_html(
        r#"<html><body>
            <input id="txt" type="text" style="width:80px;height:20px">
            <textarea id="area" style="width:80px;height:40px"></textarea>
            <div id="out" style="width:40px;height:20px">out</div>
        </body></html>"#,
    );
    let txt = harness.node("#txt");
    let area = harness.node("#area");

    assert!(click_form_events(&mut harness, "#txt").is_empty());
    let typed = type_chars(&mut harness, "hi");
    assert!(typed.iter().all(|e| e.name == "input"));

    let focus_area = click_form_events(&mut harness, "#area");
    assert_eq!(focus_area.len(), 1);
    assert_eq!(
        (
            focus_area[0].name,
            focus_area[0].target,
            &focus_area[0].value[..]
        ),
        ("change", txt, "hi")
    );

    type_chars(&mut harness, "a");
    let newline = press_key(&mut harness, Key::Named(NamedKey::Enter));
    assert_eq!(newline.len(), 1);
    assert_eq!(newline[0].name, "input");
    type_chars(&mut harness, "b");

    let blur_area = click_form_events(&mut harness, "#out");
    assert_eq!(blur_area.len(), 1);
    assert_eq!(
        (
            blur_area[0].name,
            blur_area[0].target,
            &blur_area[0].value[..]
        ),
        ("change", area, "a\nb")
    );
}

#[test]
fn reverting_text_input_before_blur_dispatches_no_change() {
    let mut harness = Harness::from_html(
        r#"<html><body>
            <input id="txt" type="text" value="init" style="width:80px;height:20px">
            <div id="out" style="width:40px;height:20px">out</div>
        </body></html>"#,
    );

    click_form_events(&mut harness, "#txt");
    press_key(&mut harness, Key::Named(NamedKey::Home));
    type_chars(&mut harness, "x");
    press_key(&mut harness, Key::Named(NamedKey::ArrowLeft));
    let reverted = press_key(&mut harness, Key::Named(NamedKey::Delete));
    assert_eq!(reverted.len(), 1);
    assert_eq!(
        (reverted[0].name, &reverted[0].value[..]),
        ("input", "init")
    );

    assert!(click_form_events(&mut harness, "#out").is_empty());
}

#[test]
fn single_line_input_commits_change_on_enter_without_duplicate_on_blur() {
    let mut harness = Harness::from_html(
        r#"<html><body>
            <input id="txt" type="text" style="width:80px;height:20px">
            <div id="out" style="width:40px;height:20px">out</div>
        </body></html>"#,
    );
    let txt = harness.node("#txt");

    click_form_events(&mut harness, "#txt");
    type_chars(&mut harness, "abc");

    let submit = press_key(&mut harness, Key::Named(NamedKey::Enter));
    assert_eq!(submit.len(), 1);
    assert_eq!(
        (submit[0].name, submit[0].target, &submit[0].value[..]),
        ("change", txt, "abc")
    );

    assert!(press_key(&mut harness, Key::Named(NamedKey::Enter)).is_empty());
    assert!(click_form_events(&mut harness, "#out").is_empty());
}
