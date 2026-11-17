//! What the user may do to a text control's value: `readonly` controls can't be edited,
//! password fields can't be copied from, a value set from code puts the caret at its end, and
//! line breaks pasted into a single-line input don't end up in its value.

use blitz_dom::{NodeId, QualName, local_name, ns};
use blitz_test_harness::Harness;
use blitz_traits::events::UiEvent;
use blitz_traits::shell::{ClipboardError, ShellProvider};
use keyboard_types::{Key, Modifiers, NamedKey};
use std::sync::{Arc, Mutex};

#[cfg(target_os = "macos")]
const ACTION: Modifiers = Modifiers::META;
#[cfg(not(target_os = "macos"))]
const ACTION: Modifiers = Modifiers::CONTROL;

#[derive(Default)]
struct ClipboardShell {
    clipboard: Mutex<Option<String>>,
}
impl ShellProvider for ClipboardShell {
    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        *self.clipboard.lock().unwrap() = Some(text);
        Ok(())
    }
}

fn with_clipboard(html: &str) -> (Harness, Arc<ClipboardShell>) {
    let mut harness = Harness::from_html(html);
    let shell = Arc::new(ClipboardShell::default());
    harness.base_mut().set_shell_provider(shell.clone());
    (harness, shell)
}

fn value(harness: &Harness, node: NodeId) -> String {
    harness
        .base()
        .get_node(node)
        .unwrap()
        .element_data()
        .unwrap()
        .text_input_data()
        .unwrap()
        .editor
        .text()
        .to_string()
}

fn shortcut(key: &str) -> UiEvent {
    UiEvent::KeyDown(blitz_test_harness::key_event(
        Key::Character(key.into()),
        blitz_traits::events::KeyState::Pressed,
        ACTION,
    ))
}

fn typing(text: &str) -> Vec<UiEvent> {
    text.chars()
        .map(|c| {
            UiEvent::KeyDown(blitz_test_harness::key_event(
                Key::Character(c.to_string()),
                blitz_traits::events::KeyState::Pressed,
                Modifiers::empty(),
            ))
        })
        .collect()
}

fn named(key: NamedKey) -> UiEvent {
    UiEvent::KeyDown(blitz_test_harness::key_event(
        Key::Named(key),
        blitz_traits::events::KeyState::Pressed,
        Modifiers::empty(),
    ))
}

#[test]
fn readonly_text_controls_cannot_be_edited_but_can_be_copied() {
    for (html, original) in [
        (r#"<input readonly autofocus value="fixed">"#, "fixed"),
        (
            "<textarea readonly autofocus>fixed\ntext</textarea>",
            "fixed\ntext",
        ),
    ] {
        let (mut harness, shell) = with_clipboard(&format!("<html><body>{html}</body></html>"));
        let control = harness.focused().expect("readonly controls are focusable");

        let mut events = typing("abc");
        events.extend([
            named(NamedKey::Backspace),
            named(NamedKey::Delete),
            named(NamedKey::Enter),
            UiEvent::Paste("pasted".into()),
            shortcut("a"),
            shortcut("x"),
            shortcut("v"),
        ]);
        let recorded = harness.dispatch_recorded(events);
        assert!(
            !recorded.iter().any(|e| e == "input"),
            "{html}: {recorded:?}"
        );
        assert_eq!(value(&harness, control), original, "{html}");

        // Selecting and copying still work.
        harness.dispatch_recorded([shortcut("a"), shortcut("c")]);
        assert_eq!(shell.clipboard.lock().unwrap().as_deref(), Some(original));
    }
}

#[test]
fn read_only_and_read_write_pseudo_classes() {
    let mut harness = Harness::from_html(
        r#"<html><head><style>
            input, textarea { box-sizing: border-box }
            input:read-only { width: 100px }
            input:read-write { width: 200px }
        </style></head><body>
            <input id=text>
            <input id=ro readonly>
            <input id=disabled disabled>
            <input id=checkbox type=checkbox>
            <textarea id=area></textarea>
            <textarea id=ro-area readonly></textarea>
            <div id=div></div>
        </body></html>"#,
    );
    let read_write = harness.query_all(":read-write");
    assert_eq!(read_write, [harness.node("#text"), harness.node("#area")]);
    for id in ["#ro", "#disabled", "#checkbox", "#ro-area", "#div", "body"] {
        assert!(
            harness.query_all(":read-only").contains(&harness.node(id)),
            "{id} should match :read-only"
        );
    }
    assert_eq!(harness.layout_rect("#ro").width, 100.0);
    assert_eq!(harness.layout_rect("#text").width, 200.0);

    // Toggling `readonly` restyles the control.
    let readonly = QualName::new(None, ns!(), local_name!("readonly"));
    let (text, ro) = (harness.node("#text"), harness.node("#ro"));
    harness
        .base_mut()
        .mutate()
        .set_attribute(text, readonly.clone(), "");
    harness.base_mut().mutate().clear_attribute(ro, readonly);
    harness.pump();
    assert_eq!(harness.layout_rect("#text").width, 100.0);
    assert_eq!(harness.layout_rect("#ro").width, 200.0);
}

#[test]
fn password_inputs_cannot_be_copied_or_cut() {
    let (mut harness, shell) = with_clipboard(
        r#"<html><body><input type=password autofocus value="secret"></body></html>"#,
    );
    let input = harness.focused().unwrap();

    let recorded = harness.dispatch_recorded([shortcut("a"), shortcut("c"), shortcut("x")]);
    assert!(!recorded.iter().any(|e| e == "input"), "{recorded:?}");
    assert_eq!(*shell.clipboard.lock().unwrap(), None);
    assert_eq!(value(&harness, input), "secret");

    // Pasting over the selection still works.
    harness.dispatch_recorded([UiEvent::Paste("new".into())]);
    assert_eq!(value(&harness, input), "new");
}

#[test]
fn setting_the_value_moves_the_caret_to_the_end() {
    let mut harness =
        Harness::from_html(r#"<html><body><input autofocus value="hello world"></body></html>"#);
    let input = harness.focused().unwrap();

    // Caret after "hello".
    let mut events = vec![named(NamedKey::Home)];
    events.extend((0..5).map(|_| named(NamedKey::ArrowRight)));
    harness.dispatch_recorded(events);

    let value_attr = QualName::new(None, ns!(), local_name!("value"));
    harness
        .base_mut()
        .mutate()
        .set_attribute(input, value_attr.clone(), "goodbye");
    harness.dispatch_recorded(typing("!"));
    assert_eq!(value(&harness, input), "goodbye!");

    // A new value also clears the selection.
    harness.dispatch_recorded([shortcut("a")]);
    harness
        .base_mut()
        .mutate()
        .set_attribute(input, value_attr.clone(), "abc");
    harness.dispatch_recorded(typing("d"));
    assert_eq!(value(&harness, input), "abcd");

    // Setting the value the input already has leaves the caret where it is.
    harness.dispatch_recorded([named(NamedKey::Home)]);
    harness
        .base_mut()
        .mutate()
        .set_attribute(input, value_attr, "abcd");
    harness.dispatch_recorded(typing("x"));
    assert_eq!(value(&harness, input), "xabcd");
}

#[test]
fn pasting_line_breaks_into_a_single_line_input() {
    let mut harness = Harness::from_html("<html><body><input autofocus></body></html>");
    let input = harness.focused().unwrap();
    harness.dispatch_recorded([UiEvent::Paste("one\ntwo\r\nthree\rfour\n\n".into())]);
    assert_eq!(value(&harness, input), "one two three four");

    let mut harness =
        Harness::from_html("<html><body><textarea autofocus></textarea></body></html>");
    let textarea = harness.focused().unwrap();
    harness.dispatch_recorded([UiEvent::Paste("one\ntwo\n".into())]);
    assert_eq!(value(&harness, textarea), "one\ntwo\n");
}
