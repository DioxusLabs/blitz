//! The `maxlength` attribute limits the length of user-entered text in text controls.

use std::sync::Arc;

use blitz_test_harness::Harness;
use blitz_traits::events::BlitzImeEvent;
use blitz_traits::shell::{ClipboardError, ShellProvider};
use keyboard_types::{Key, Modifiers};

#[cfg(target_os = "macos")]
const ACTION_MOD: Modifiers = Modifiers::SUPER;
#[cfg(not(target_os = "macos"))]
const ACTION_MOD: Modifiers = Modifiers::CONTROL;

fn harness(control: &str) -> Harness {
    let mut harness = Harness::from_html(&format!(
        r#"<html><body style="margin:0">{control}</body></html>"#
    ));
    harness.click("#control");
    harness
}

fn value(harness: &Harness) -> String {
    let node_id = harness.node("#control");
    let doc = harness.base();
    let element = doc.get_node(node_id).unwrap().element_data().unwrap();
    element
        .text_input_data()
        .unwrap()
        .editor
        .raw_text()
        .to_string()
}

struct Clipboard(&'static str);

impl ShellProvider for Clipboard {
    fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
        Ok(self.0.to_string())
    }
}

#[test]
fn typing_stops_at_maxlength() {
    let mut harness = harness(r#"<input id="control" maxlength="3">"#);
    harness.type_text("abcdef");
    assert_eq!(value(&harness), "abc");
}

#[test]
fn textarea_typing_stops_at_maxlength() {
    let mut harness = harness(r#"<textarea id="control" maxlength="3"></textarea>"#);
    harness.type_text("ab");
    harness.press(Key::Enter);
    harness.press(Key::Enter);
    harness.type_text("c");
    assert_eq!(value(&harness), "ab\n");
}

#[test]
fn typing_replaces_selection_at_maxlength() {
    let mut harness = harness(r#"<input id="control" maxlength="3">"#);
    harness.type_text("abc");
    harness.press_with(Key::Character("a".into()), ACTION_MOD);
    harness.type_text("xy");
    assert_eq!(value(&harness), "xy");
}

#[test]
fn paste_is_truncated() {
    let mut harness = harness(r#"<input id="control" maxlength="4">"#);
    harness
        .base_mut()
        .set_shell_provider(Arc::new(Clipboard("abcdef")));
    harness.type_text("z");
    harness.press_with(Key::Character("v".into()), ACTION_MOD);
    assert_eq!(value(&harness), "zabc");
}

#[test]
fn ime_commit_is_truncated_without_splitting_characters() {
    // U+1F600 is two UTF-16 code units, so it doesn't fit in the one remaining unit.
    let mut harness = harness(r#"<input id="control" maxlength="3">"#);
    harness.ime(BlitzImeEvent::Commit("a\u{1F600}b".into()));
    harness.ime(BlitzImeEvent::Commit("\u{1F600}".into()));
    assert_eq!(value(&harness), "a\u{1F600}");
}

#[test]
fn initial_value_is_not_truncated() {
    let mut harness = harness(r#"<input id="control" maxlength="2" value="abcd">"#);
    harness.type_text("e");
    assert_eq!(value(&harness), "abcd");
}

#[test]
fn maxlength_is_parsed_as_non_negative_integer() {
    let mut harness = harness(r#"<input id="control" maxlength=" +2px">"#);
    harness.type_text("abc");
    assert_eq!(value(&harness), "ab");

    for maxlength in ["-1", "abc", ""] {
        let mut harness = harness_with_maxlength(maxlength);
        harness.type_text("abc");
        assert_eq!(value(&harness), "abc", "maxlength={maxlength:?}");
    }
}

fn harness_with_maxlength(maxlength: &str) -> Harness {
    harness(&format!(r#"<input id="control" maxlength="{maxlength}">"#))
}

#[test]
fn maxlength_does_not_apply_to_number_inputs() {
    let mut harness = harness(r#"<input id="control" type="number" maxlength="1">"#);
    harness.type_text("1234");
    assert_eq!(value(&harness), "1234");
}
