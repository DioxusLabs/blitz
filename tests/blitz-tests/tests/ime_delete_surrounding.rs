//! Input methods (Android soft keyboards, Wayland) delete text with `DeleteSurrounding`.

use blitz_test_harness::Harness;
use blitz_traits::events::{BlitzImeEvent, UiEvent};
use keyboard_types::{Key, NamedKey};

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
fn delete_surrounding_deletes_before_the_caret_and_fires_input() {
    let mut harness =
        Harness::from_html("<html><body><input autofocus value=\"hello\"></body></html>");
    harness.press(Key::Named(NamedKey::End));

    let events = harness.dispatch_recorded([UiEvent::Ime(BlitzImeEvent::DeleteSurrounding {
        before_bytes: 2,
        after_bytes: 0,
    })]);

    assert_eq!(value(&harness), "hel");
    // The IME event itself reaches the page as `composition`, followed by the edit's `input`.
    assert_eq!(events, ["composition", "input"]);
}
