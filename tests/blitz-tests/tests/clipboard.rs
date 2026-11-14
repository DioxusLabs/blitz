use blitz_test_harness::{Harness, key_event};
use blitz_traits::events::{KeyState, UiEvent};
use keyboard_types::{Key, Modifiers};

#[cfg(target_os = "macos")]
const ACTION: Modifiers = Modifiers::META;
#[cfg(not(target_os = "macos"))]
const ACTION: Modifiers = Modifiers::CONTROL;

/// A shortcut's key down and key up.
fn shortcut(key: &str) -> [UiEvent; 2] {
    [
        UiEvent::KeyDown(key_event(
            Key::Character(key.into()),
            KeyState::Pressed,
            ACTION,
        )),
        UiEvent::KeyUp(key_event(
            Key::Character(key.into()),
            KeyState::Released,
            ACTION,
        )),
    ]
}

fn inputs(events: &[String]) -> usize {
    events.iter().filter(|name| *name == "input").count()
}

fn value(harness: &Harness) -> String {
    let input = harness.focused().unwrap();
    harness
        .base()
        .get_node(input)
        .unwrap()
        .element_data()
        .unwrap()
        .text_input_data()
        .unwrap()
        .editor
        .text()
        .to_string()
}

#[test]
fn paste_dispatches_input_only_with_focused_text_input() {
    let mut harness = Harness::from_html("<html><body><input autofocus></body></html>");
    let input = harness.focused().unwrap();
    let events = harness.dispatch_recorded([UiEvent::Paste("from clipboard".into())]);
    assert_eq!(events, ["input"]);
    assert_eq!(
        harness
            .base()
            .get_node(input)
            .unwrap()
            .element_data()
            .unwrap()
            .text_input_data()
            .unwrap()
            .editor
            .text(),
        "from clipboard"
    );
    let mut harness = Harness::from_html("<html><body><input></body></html>");
    assert!(
        harness
            .dispatch_recorded([UiEvent::Paste("ignored".into())])
            .is_empty()
    );
    let mut harness =
        Harness::from_html("<html><body><button autofocus>Button</button></body></html>");
    assert_eq!(harness.focused(), Some(harness.node("button")));
    assert!(
        harness
            .dispatch_recorded([UiEvent::Paste("ignored".into())])
            .is_empty()
    );
}

#[test]
fn copy_dispatches_no_input_and_cut_only_when_it_removes_text() {
    let mut harness =
        Harness::from_html(r#"<html><body><input autofocus value="hello"></body></html>"#);

    // Nothing selected: copy and cut leave the value alone.
    assert_eq!(inputs(&harness.dispatch_recorded(shortcut("c"))), 0);
    assert_eq!(inputs(&harness.dispatch_recorded(shortcut("x"))), 0);
    assert_eq!(value(&harness), "hello");

    // Everything selected: copy still isn't an edit, cut is one.
    harness.dispatch_recorded(shortcut("a"));
    assert_eq!(inputs(&harness.dispatch_recorded(shortcut("c"))), 0);
    assert_eq!(value(&harness), "hello");
    assert_eq!(inputs(&harness.dispatch_recorded(shortcut("x"))), 1);
    assert_eq!(value(&harness), "");
}
