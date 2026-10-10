//! Editing an `<input>` and a `<textarea>` through key, IME and clipboard events, under whichever
//! text backend the build selects: typing, deletion, word motion, selection, the clipboard, undo
//! and text an input method is composing.

use std::sync::{Arc, Mutex};

use blitz_dom::text::{BACKEND, EditableText as _, TextBackend};
use blitz_dom::{DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_test_harness::Harness;
use blitz_traits::events::BlitzImeEvent;
use blitz_traits::shell::{ClipboardError, ColorScheme, ShellProvider, Viewport};
use keyboard_types::{Key, Modifiers};

/// The modifier editing shortcuts are taken with.
const ACTION: Modifiers = if cfg!(target_os = "macos") {
    Modifiers::SUPER
} else {
    Modifiers::CONTROL
};

/// A shell whose clipboard is a string.
#[derive(Default)]
struct Clipboard(Mutex<String>);

impl ShellProvider for Clipboard {
    fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        *self.0.lock().unwrap() = text;
        Ok(())
    }
}

/// A focused `<input>` holding `value`, with the caret at its end, and the clipboard its
/// document is handed.
fn input(value: &str) -> (Harness, NodeId, Arc<Clipboard>) {
    control(&format!(
        r#"<input id="field" type="text" value="{value}" style="width:400px; height:24px; font-size:16px">"#
    ))
}

/// A focused `<textarea>` holding `value`.
fn textarea(value: &str) -> (Harness, NodeId, Arc<Clipboard>) {
    control(&format!(
        r#"<textarea id="field" style="width:400px; height:200px; font-size:16px">{value}</textarea>"#
    ))
}

fn control(markup: &str) -> (Harness, NodeId, Arc<Clipboard>) {
    let clipboard = Arc::new(Clipboard::default());
    let html = format!(r#"<html><body style="margin:0">{markup}</body></html>"#);
    let doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            shell_provider: Some(clipboard.clone() as _),
            ..Default::default()
        },
    );
    let mut harness = Harness::wrap(doc);
    harness.pump();
    harness.click("#field");
    let node = harness.node("#field");
    harness.press_with(Key::End, ACTION);
    (harness, node, clipboard)
}

/// The control's text, as a form submits it.
fn value(harness: &Harness, node: NodeId) -> String {
    with_editor(harness, node, |editor| editor.text().into_owned())
}

/// The control's text with any text an input method is composing.
fn raw_value(harness: &Harness, node: NodeId) -> String {
    with_editor(harness, node, |editor| editor.raw_text().to_owned())
}

fn selection(harness: &Harness, node: NodeId) -> std::ops::Range<usize> {
    with_editor(harness, node, |editor| editor.selection())
}

fn selected(harness: &Harness, node: NodeId) -> Option<String> {
    with_editor(harness, node, |editor| {
        editor.selected_text().map(str::to_owned)
    })
}

fn caret_shown(harness: &Harness, node: NodeId) -> bool {
    with_editor(harness, node, |editor| editor.caret_rect().is_some())
}

fn with_editor<T>(
    harness: &Harness,
    node: NodeId,
    read: impl FnOnce(&blitz_dom::text::TextEditor) -> T,
) -> T {
    let doc = harness.base();
    let element = doc.get_node(node).unwrap().element_data().unwrap();
    read(&element.text_input_data().unwrap().editor)
}

/// Backspace, which macOS takes through its standard key bindings instead.
fn backspace(harness: &mut Harness, modifiers: Modifiers) {
    if cfg!(target_os = "macos") {
        let command = if modifiers.contains(Modifiers::ALT) {
            "deleteWordBackward:"
        } else {
            "deleteBackward:"
        };
        harness.dispatch(blitz_traits::events::UiEvent::AppleStandardKeybinding(
            command.into(),
        ));
        harness.pump();
    } else {
        harness.press_with(Key::Backspace, modifiers);
    }
}

/// The modifier word motion and word deletion are taken with.
const WORD: Modifiers = if cfg!(target_os = "macos") {
    Modifiers::ALT
} else {
    Modifiers::CONTROL
};

#[test]
fn typing_inserts_at_the_caret() {
    let (mut harness, node, _) = input("world");
    harness.press(Key::Home);
    harness.type_text("hello ");
    assert_eq!(value(&harness, node), "hello world");
    assert_eq!(selection(&harness, node), 6..6);
}

#[test]
fn enter_inserts_a_line_break_in_a_textarea() {
    let (mut harness, node, _) = textarea("one");
    harness.press(Key::Enter);
    harness.type_text("two");
    assert_eq!(value(&harness, node), "one\ntwo");
}

#[test]
fn backspace_and_delete_remove_a_character() {
    let (mut harness, node, _) = input("hello");
    backspace(&mut harness, Modifiers::empty());
    assert_eq!(value(&harness, node), "hell");
    harness.press(Key::Home);
    harness.press(Key::Delete);
    assert_eq!(value(&harness, node), "ell");
    assert_eq!(selection(&harness, node), 0..0);
}

#[test]
fn deleting_at_the_ends_changes_nothing() {
    let (mut harness, node, _) = input("ab");
    harness.press(Key::Delete);
    harness.press(Key::Home);
    backspace(&mut harness, Modifiers::empty());
    assert_eq!(value(&harness, node), "ab");
    assert_eq!(selection(&harness, node), 0..0);
}

/// Backspace after a combining mark: winkin takes the whole grapheme, Parley the mark alone.
#[test]
fn backspace_after_a_combining_mark() {
    let (mut harness, node, _) = input("ae\u{301}");
    backspace(&mut harness, Modifiers::empty());
    let expected = match BACKEND {
        TextBackend::Winkin => "a",
        TextBackend::Parley => "ae",
    };
    assert_eq!(value(&harness, node), expected);
}

#[test]
fn word_motion_moves_by_words() {
    let (mut harness, node, _) = input("one two three");
    harness.press_with(Key::ArrowLeft, WORD);
    assert_eq!(selection(&harness, node), 8..8);
    harness.press_with(Key::ArrowLeft, WORD);
    assert_eq!(selection(&harness, node), 4..4);
    harness.press_with(Key::ArrowLeft, WORD | Modifiers::SHIFT);
    assert_eq!(selected(&harness, node).as_deref(), Some("one "));
}

#[test]
fn word_deletion_removes_the_word_before_the_caret() {
    let (mut harness, node, _) = input("one two");
    backspace(&mut harness, WORD);
    assert_eq!(value(&harness, node), "one ");
}

#[test]
fn shift_motion_extends_and_typing_replaces_the_selection() {
    let (mut harness, node, _) = input("hello world");
    for _ in 0..5 {
        harness.press_with(Key::ArrowLeft, Modifiers::SHIFT);
    }
    assert_eq!(selected(&harness, node).as_deref(), Some("world"));
    harness.type_text("there");
    assert_eq!(value(&harness, node), "hello there");
}

#[test]
fn select_all_selects_the_whole_text() {
    let (mut harness, node, _) = textarea("one\ntwo");
    harness.press_with(Key::Character("a".into()), ACTION);
    assert_eq!(selected(&harness, node).as_deref(), Some("one\ntwo"));
    harness.press(Key::ArrowLeft);
    assert_eq!(selection(&harness, node), 0..0);
}

#[test]
fn copy_cut_and_paste_go_through_the_clipboard() {
    let (mut harness, node, clipboard) = input("hello world");
    harness.press_with(Key::ArrowLeft, WORD | Modifiers::SHIFT);
    harness.press_with(Key::Character("c".into()), ACTION);
    assert_eq!(*clipboard.0.lock().unwrap(), "world");
    assert_eq!(value(&harness, node), "hello world");

    harness.press_with(Key::Character("x".into()), ACTION);
    assert_eq!(value(&harness, node), "hello ");

    harness.press(Key::Home);
    harness.press_with(Key::Character("v".into()), ACTION);
    assert_eq!(value(&harness, node), "worldhello ");
    assert_eq!(selection(&harness, node), 5..5);
}

#[test]
fn paste_replaces_the_selection() {
    let (mut harness, node, clipboard) = input("abc");
    *clipboard.0.lock().unwrap() = "xyz".into();
    harness.press_with(Key::Character("a".into()), ACTION);
    harness.press_with(Key::Character("v".into()), ACTION);
    assert_eq!(value(&harness, node), "xyz");
}

/// Undo and redo, which the winkin editor keeps a history for, and Parley's does not.
#[test]
fn undo_and_redo_step_through_edits() {
    let (mut harness, node, _) = input("");
    harness.type_text("ab");
    backspace(&mut harness, Modifiers::empty());
    assert_eq!(value(&harness, node), "a");

    harness.press_with(Key::Character("z".into()), ACTION);
    match BACKEND {
        TextBackend::Winkin => {
            assert_eq!(value(&harness, node), "ab");
            assert_eq!(selection(&harness, node), 2..2);
            harness.press_with(Key::Character("z".into()), ACTION);
            assert_eq!(value(&harness, node), "a");
            harness.press_with(Key::Character("Z".into()), ACTION | Modifiers::SHIFT);
            assert_eq!(value(&harness, node), "ab");
            harness.press(Key::Redo);
            assert_eq!(value(&harness, node), "a");
            harness.press(Key::Undo);
            assert_eq!(value(&harness, node), "ab");
        }
        TextBackend::Parley => assert_eq!(value(&harness, node), "a"),
    }
}

#[test]
fn undo_restores_a_replaced_selection() {
    let (mut harness, node, _) = input("hello");
    harness.press_with(Key::Character("a".into()), ACTION);
    harness.type_text("x");
    harness.press_with(Key::Character("z".into()), ACTION);
    if BACKEND == TextBackend::Winkin {
        assert_eq!(value(&harness, node), "hello");
        assert_eq!(selected(&harness, node).as_deref(), Some("hello"));
        // A new edit drops what could be redone.
        harness.press(Key::End);
        harness.type_text("!");
        harness.press(Key::Redo);
        assert_eq!(value(&harness, node), "hello!");
    }
}

#[test]
fn composing_text_is_laid_out_until_it_is_committed() {
    let (mut harness, node, _) = input("abc");
    harness.ime(BlitzImeEvent::Preedit("に".into(), Some((3, 3))));
    harness.ime(BlitzImeEvent::Preedit("日本".into(), Some((6, 6))));
    assert_eq!(raw_value(&harness, node), "abc日本");
    assert_eq!(value(&harness, node), "abc");
    assert_eq!(selection(&harness, node), 9..9);
    assert!(caret_shown(&harness, node));
    assert_eq!(selected(&harness, node), None);

    // Without a cursor, the input method hides the caret.
    harness.ime(BlitzImeEvent::Preedit("日本".into(), None));
    assert!(!caret_shown(&harness, node));

    harness.ime(BlitzImeEvent::Preedit(String::new(), None));
    harness.ime(BlitzImeEvent::Commit("日本".into()));
    assert_eq!(raw_value(&harness, node), "abc日本");
    assert_eq!(value(&harness, node), "abc日本");
    assert_eq!(selection(&harness, node), 9..9);
    assert!(caret_shown(&harness, node));
}

#[test]
fn cancelled_composing_text_leaves_the_text_as_it_was() {
    let (mut harness, node, _) = input("abc");
    harness.press(Key::ArrowLeft);
    harness.ime(BlitzImeEvent::Preedit("x".into(), Some((1, 1))));
    assert_eq!(raw_value(&harness, node), "abxc");
    harness.ime(BlitzImeEvent::Preedit(String::new(), None));
    assert_eq!(raw_value(&harness, node), "abc");
    assert_eq!(selection(&harness, node), 2..2);

    harness.ime(BlitzImeEvent::Preedit("x".into(), Some((1, 1))));
    harness.ime(BlitzImeEvent::Disabled);
    assert_eq!(raw_value(&harness, node), "abc");
}

/// What composing does to a selection: the winkin editor returns to it where composing is
/// cancelled, and a commit replaces it as one change to undo.
#[test]
fn composing_over_a_selection_restores_it_or_replaces_it() {
    if BACKEND != TextBackend::Winkin {
        return;
    }
    let (mut harness, node, _) = input("abc");
    harness.press_with(Key::Character("a".into()), ACTION);
    harness.ime(BlitzImeEvent::Preedit("に".into(), Some((3, 3))));
    assert_eq!(raw_value(&harness, node), "に");
    harness.ime(BlitzImeEvent::Preedit(String::new(), None));
    assert_eq!(selected(&harness, node).as_deref(), Some("abc"));

    harness.ime(BlitzImeEvent::Preedit("日本".into(), Some((6, 6))));
    harness.ime(BlitzImeEvent::Preedit(String::new(), None));
    harness.ime(BlitzImeEvent::Commit("日本語".into()));
    assert_eq!(value(&harness, node), "日本語");
    harness.press_with(Key::Character("z".into()), ACTION);
    assert_eq!(selected(&harness, node).as_deref(), Some("abc"));
}

/// The composing text is underlined: winkin lays it out in a box of its own, which decorates
/// the text before it is painted.
#[cfg(all(feature = "winkin", not(feature = "parley")))]
#[test]
fn composing_text_is_underlined() {
    use blitz_dom::text::winkin::COMPOSE_KEY;
    use blitz_dom::text::winkin::winkin::paint::{Decorates, Paint};

    let (mut harness, node, _) = input("abc");
    harness.ime(BlitzImeEvent::Preedit("xy".into(), Some((2, 2))));
    let compose_key = COMPOSE_KEY | node.as_u64();
    let bars = with_editor(&harness, node, |editor| {
        let layout = editor.text_layout().layout().unwrap();
        layout
            .lines()
            .flat_map(|line| {
                line.paints(|key| {
                    if key.0 == compose_key {
                        Decorates::BeforeText
                    } else {
                        Decorates::None
                    }
                })
                .filter_map(|paint| match paint {
                    Paint::DecorationBeforeText(bar) => {
                        let inline = bar.inline();
                        Some((bar.key().0, inline.left, inline.right))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        bars.len(),
        1,
        "one underline under the composing text: {bars:?}"
    );
    let (key, left, right) = bars[0];
    assert_eq!(key, compose_key);
    assert!(left > 0.0 && right > left);

    harness.ime(BlitzImeEvent::Preedit(String::new(), None));
    let composing = with_editor(&harness, node, |editor| editor.composition_range());
    assert_eq!(composing, None);
}
