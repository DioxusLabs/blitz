//! `<input type=password>`, as Chrome edits it: its text is never copied or cut, a word motion or
//! a word deletion takes the whole text, and a double click selects it all. Under winkin the
//! text is drawn as a disc for each grapheme, with the caret and the selection in the text, and
//! optionally the character typed last is shown in the clear for a while, as iOS and Android
//! show it.

use std::sync::{Arc, Mutex};

use blitz_dom::text::EditableText as _;
use blitz_dom::{DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_test_harness::Harness;
use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::shell::{ClipboardError, ColorScheme, ShellProvider, Viewport};
use keyboard_types::{Key, Modifiers};

/// The modifier editing shortcuts and word motions are taken with.
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

/// A navigation provider that keeps the URL a form submits to.
#[derive(Default)]
struct Navigations(Mutex<Vec<String>>);

impl NavigationProvider for Navigations {
    fn navigate_to(&self, options: NavigationOptions) {
        self.0.lock().unwrap().push(options.url.to_string());
    }
}

/// A focused password field holding `value`, with the caret at its end, and the clipboard its
/// document is handed. A typed character shows in the clear for `reveal` where it is set.
fn password_with(
    value: &str,
    reveal: Option<std::time::Duration>,
) -> (Harness, NodeId, Arc<Clipboard>) {
    let clipboard = Arc::new(Clipboard::default());
    let html = format!(
        r#"<html><body style="margin:0"><input id="field" type="password" value="{value}" style="width:400px; height:24px; font-size:16px"></body></html>"#
    );
    let doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            shell_provider: Some(clipboard.clone() as _),
            reveal_typed_password_character: reveal,
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

fn password(value: &str) -> (Harness, NodeId, Arc<Clipboard>) {
    password_with(value, None)
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

fn value(harness: &Harness, node: NodeId) -> String {
    with_editor(harness, node, |editor| editor.text().into_owned())
}

fn selection(harness: &Harness, node: NodeId) -> std::ops::Range<usize> {
    with_editor(harness, node, |editor| editor.selection())
}

/// Moves the caret to byte `at` of the field's text.
fn caret_at(harness: &mut Harness, node: NodeId, at: usize) {
    harness.press(Key::Home);
    while selection(harness, node).start < at {
        harness.press(Key::ArrowRight);
    }
    assert_eq!(selection(harness, node), at..at);
}

/// Chrome copies and cuts nothing from a password field, and fires no event for it, but pastes
/// into it.
#[test]
fn copy_and_cut_are_blocked() {
    let (mut harness, node, clipboard) = password("secret");
    *clipboard.0.lock().unwrap() = "before".into();
    harness.press_with(Key::Character("a".into()), ACTION);
    assert_eq!(selection(&harness, node), 0..6);
    harness.press_with(Key::Character("c".into()), ACTION);
    assert_eq!(*clipboard.0.lock().unwrap(), "before");
    harness.press_with(Key::Character("x".into()), ACTION);
    assert_eq!(*clipboard.0.lock().unwrap(), "before");
    assert_eq!(value(&harness, node), "secret");
    assert_eq!(selection(&harness, node), 0..6);
    harness.press_with(Key::Character("v".into()), ACTION);
    assert_eq!(value(&harness, node), "before");
}

/// A word motion goes to the start or the end of the text, as Chrome's Ctrl+arrows do in a
/// password field, where `hello world foo.bar baz` from offset 8 goes to 23 and to 0.
#[test]
fn word_motion_goes_to_either_end() {
    let (mut harness, node, _) = password("hello world foo.bar baz");
    caret_at(&mut harness, node, 8);
    harness.press_with(Key::ArrowRight, ACTION);
    assert_eq!(selection(&harness, node), 23..23);
    caret_at(&mut harness, node, 8);
    harness.press_with(Key::ArrowLeft, ACTION);
    assert_eq!(selection(&harness, node), 0..0);
    caret_at(&mut harness, node, 8);
    harness.press_with(Key::ArrowRight, ACTION | Modifiers::SHIFT);
    assert_eq!(selection(&harness, node), 8..23);
}

/// A word deletion deletes to the start or the end of the text: in Chrome, Ctrl+Backspace at
/// offset 8 leaves `rld foo.bar baz`, and Ctrl+Delete leaves `hello wo`.
#[cfg(not(target_os = "macos"))]
#[test]
fn word_deletion_goes_to_either_end() {
    let (mut harness, node, _) = password("hello world foo.bar baz");
    caret_at(&mut harness, node, 8);
    harness.press_with(Key::Backspace, ACTION);
    assert_eq!(value(&harness, node), "rld foo.bar baz");
    assert_eq!(selection(&harness, node), 0..0);
    let (mut harness, node, _) = password("hello world foo.bar baz");
    caret_at(&mut harness, node, 8);
    harness.press_with(Key::Delete, ACTION);
    assert_eq!(value(&harness, node), "hello wo");
}

/// A double click selects the whole text, as in Chrome.
#[test]
fn a_double_click_selects_everything() {
    let (mut harness, node, _) = password("hello world");
    harness.press(Key::Home);
    let (x, y) = harness.center_of("#field");
    harness.click_at(x - 150.0, y);
    harness.click_at(x - 150.0, y);
    assert_eq!(selection(&harness, node), 0..11);
}

/// The value a form submits is the text, not what is drawn.
#[test]
fn a_form_submits_the_text() {
    let navigations = Arc::new(Navigations::default());
    let html = r#"<html><body><form action="http://example.com/login"><input id="field" type="password" name="pw" value=""></form></body></html>"#;
    let doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            navigation_provider: Some(navigations.clone() as _),
            ..Default::default()
        },
    );
    let mut harness = Harness::wrap(doc);
    harness.pump();
    harness.click("#field");
    harness.type_text("hunter2");
    harness.press(Key::Enter);
    assert_eq!(
        *navigations.0.lock().unwrap(),
        ["http://example.com/login?pw=hunter2"]
    );
}

/// winkin's layout of a password field, and the character shown in the clear.
#[cfg(all(feature = "winkin", not(feature = "parley")))]
mod winkin_masks {
    use std::time::Duration;

    use blitz_traits::events::BlitzImeEvent;
    use keyboard_types::Key;

    use super::*;

    /// The text the field is drawn with.
    fn drawn(harness: &Harness, node: NodeId) -> String {
        with_editor(harness, node, |editor| {
            editor.text_layout().layout().unwrap().text().to_owned()
        })
    }

    /// `count` discs.
    fn discs(count: usize) -> String {
        "\u{2022}".repeat(count)
    }

    /// Typing draws a disc for each grapheme of the text, as Chrome does: a combining sequence or
    /// an emoji is one.
    #[test]
    fn typing_draws_a_disc_a_grapheme() {
        let (mut harness, node, _) = password("");
        harness.type_text("ab e\u{301}\u{1F600}");
        assert_eq!(value(&harness, node), "ab e\u{301}\u{1F600}");
        assert_eq!(drawn(&harness, node), discs(5));
    }

    /// The caret steps over a grapheme at a time, at the grapheme's ends in the text, as Chrome's
    /// arrow keys do in a password field.
    #[test]
    fn the_caret_steps_a_grapheme_at_a_time() {
        let (mut harness, node, _) = password("ae\u{301}\u{1F600}b");
        harness.press(Key::Home);
        let mut stops = vec![selection(&harness, node).start];
        for _ in 0..4 {
            harness.press(Key::ArrowRight);
            stops.push(selection(&harness, node).start);
        }
        assert_eq!(stops, [0, 1, 4, 8, 9]);
        harness.press(Key::ArrowLeft);
        assert_eq!(selection(&harness, node), 8..8);
        let caret = with_editor(&harness, node, |editor| editor.caret_rect().unwrap());
        harness.press(Key::ArrowLeft);
        let before = with_editor(&harness, node, |editor| editor.caret_rect().unwrap());
        assert!(before.x0 < caret.x0, "{before:?} {caret:?}");
    }

    /// The selection is in the text, and is drawn over the discs it covers.
    #[test]
    fn the_selection_is_in_the_text() {
        let (mut harness, node, _) = password("ae\u{301}\u{1F600}b");
        harness.press(Key::Home);
        harness.press(Key::ArrowRight);
        harness.press_with(Key::ArrowRight, Modifiers::SHIFT);
        harness.press_with(Key::ArrowRight, Modifiers::SHIFT);
        assert_eq!(selection(&harness, node), 1..8);
        let drawn = with_editor(&harness, node, |editor| editor.layout_selection_range());
        assert_eq!(drawn, 3..9);
        harness.type_text("x");
        assert_eq!(value(&harness, node), "axb");
    }

    /// A click puts the caret at the grapheme end nearest it.
    #[test]
    fn a_click_hits_a_grapheme_end() {
        let (mut harness, node, _) = password("ae\u{301}\u{1F600}b");
        let rect = harness.layout_rect("#field");
        // Past the discs: the end.
        harness.click_at(rect.x + 300.0, rect.y + 12.0);
        assert_eq!(selection(&harness, node), 9..9);
        // At the very start.
        harness.click_at(rect.x + 1.0, rect.y + 12.0);
        assert_eq!(selection(&harness, node), 0..0);
    }

    /// Text an input method composes is masked too, as Chrome masks it.
    #[test]
    fn composing_text_is_masked() {
        let (mut harness, node, _) = password("abc");
        harness.ime(BlitzImeEvent::Preedit("xy".into(), Some((2, 2))));
        assert_eq!(drawn(&harness, node), discs(5));
        harness.ime(BlitzImeEvent::Preedit(String::new(), None));
        harness.ime(BlitzImeEvent::Commit("xy".into()));
        assert_eq!(value(&harness, node), "abcxy");
        assert_eq!(drawn(&harness, node), discs(5));
    }

    /// Without the option, Chrome's desktop behavior, nothing typed is shown.
    #[test]
    fn nothing_is_shown_without_the_option() {
        let (mut harness, node, _) = password("");
        harness.type_text("abc");
        assert_eq!(drawn(&harness, node), discs(3));
        assert_eq!(harness.base().text_input_deadline(), None);
    }

    const REVEAL: Duration = Duration::from_millis(1500);

    /// With the option, the grapheme typed last is shown until the next is typed.
    #[test]
    fn the_last_typed_grapheme_is_shown() {
        let (mut harness, node, _) = password_with("", Some(REVEAL));
        harness.type_text("ab");
        assert_eq!(drawn(&harness, node), format!("{}b", discs(1)));
        harness.type_text("c");
        assert_eq!(drawn(&harness, node), format!("{}c", discs(2)));
        // Typed in the middle, it is shown where it is.
        harness.press(Key::ArrowLeft);
        assert_eq!(drawn(&harness, node), discs(3));
        harness.type_text("x");
        assert_eq!(drawn(&harness, node), format!("{}x{}", discs(2), discs(1)));
        assert_eq!(value(&harness, node), "abxc");
        assert_eq!(selection(&harness, node), 3..3);
    }

    /// A move of the caret, a deletion, a paste, a change of focus and a value set from script
    /// each mask the character shown.
    #[test]
    fn other_edits_mask_the_character_shown() {
        let (mut harness, node, clipboard) = password_with("", Some(REVEAL));
        harness.type_text("a");
        harness.press(Key::ArrowLeft);
        assert_eq!(drawn(&harness, node), discs(1));

        harness.press(Key::End);
        harness.type_text("b");
        harness.press(Key::Backspace);
        assert_eq!(drawn(&harness, node), discs(1));

        harness.type_text("c");
        *clipboard.0.lock().unwrap() = "d".into();
        harness.press_with(Key::Character("v".into()), ACTION);
        assert_eq!(drawn(&harness, node), discs(3));
        assert_eq!(harness.base().text_input_deadline(), None);

        harness.type_text("e");
        harness.base_mut().clear_focus();
        assert_eq!(drawn(&harness, node), discs(4));
    }

    /// An input method's commit of one grapheme is shown, and one of several is not.
    #[test]
    fn a_committed_grapheme_is_shown_and_more_are_not() {
        let (mut harness, node, _) = password_with("", Some(REVEAL));
        harness.ime(BlitzImeEvent::Commit("\u{65E5}".into()));
        assert_eq!(drawn(&harness, node), "\u{65E5}");
        harness.ime(BlitzImeEvent::Commit("\u{672C}\u{8A9E}".into()));
        assert_eq!(drawn(&harness, node), discs(3));
    }

    /// The character is masked once its time is up, which the document reports for a shell to
    /// wake at.
    #[test]
    fn the_character_is_masked_when_its_time_is_up() {
        let (mut harness, node, _) = password_with("", Some(REVEAL));
        let typed = std::time::Instant::now();
        harness.type_text("a");
        let deadline = harness.base().text_input_deadline().unwrap();
        assert!(deadline >= typed + REVEAL);
        assert!(
            !harness
                .base_mut()
                .expire_text_input_timers(deadline - Duration::from_millis(1))
        );
        assert_eq!(drawn(&harness, node), "a");
        assert!(harness.base_mut().expire_text_input_timers(deadline));
        assert_eq!(drawn(&harness, node), discs(1));
        assert_eq!(harness.base().text_input_deadline(), None);
        assert_eq!(value(&harness, node), "a");
    }

    /// A field whose style masks nothing shows its text: a reveal only ever shows text in a
    /// password field.
    #[test]
    fn a_text_field_is_drawn_as_it_is() {
        let html = r#"<html><body><input id="field" value="abc"></body></html>"#;
        let doc = HtmlDocument::from_html(
            html,
            DocumentConfig {
                html_parser_provider: Some(Arc::new(HtmlProvider) as _),
                reveal_typed_password_character: Some(REVEAL),
                ..Default::default()
            },
        );
        let mut harness = Harness::wrap(doc);
        harness.pump();
        harness.click("#field");
        harness.press(Key::End);
        harness.type_text("d");
        let node = harness.node("#field");
        assert_eq!(drawn(&harness, node), "abcd");
        assert_eq!(harness.base().text_input_deadline(), None);
    }
}
