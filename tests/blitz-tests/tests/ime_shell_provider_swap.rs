//! A text input can be focussed before the document has its real shell provider
//! (e.g. `autofocus` while parsing a static HTML document, before the window is
//! created). The IME requests made at that point go to the previous (dummy)
//! provider, so they must be re-sent once the real provider is set.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, ShellProvider, Viewport};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct RecordingShell {
    ime_enabled_calls: Mutex<Vec<bool>>,
    ime_cursor_areas: Mutex<Vec<(f32, f32, f32, f32)>>,
}
impl ShellProvider for RecordingShell {
    fn set_ime_enabled(&self, is_enabled: bool) {
        self.ime_enabled_calls.lock().unwrap().push(is_enabled);
    }
    fn set_ime_cursor_area(&self, x: f32, y: f32, width: f32, height: f32) {
        self.ime_cursor_areas
            .lock()
            .unwrap()
            .push((x, y, width, height));
    }
}

const INPUT_HTML: &str = r#"<!DOCTYPE html>
<html><head><style> body { margin: 0; } </style></head>
<body><input type="text" id="input"><button id="button">b</button></body></html>
"#;

fn make_doc() -> HtmlDocument {
    // No shell provider: the document starts with the dummy provider
    HtmlDocument::from_html(
        INPUT_HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    )
}

#[test]
fn ime_state_is_resent_when_focussed_before_layout() {
    let mut doc = make_doc();
    let input = doc.query_selector("#input").unwrap().unwrap();
    doc.set_focus_to(input);

    let shell = Arc::new(RecordingShell::default());
    doc.set_shell_provider(shell.clone());
    doc.resend_ime_state();
    assert_eq!(
        *shell.ime_enabled_calls.lock().unwrap(),
        vec![true],
        "the focussed text input's IME enable should be re-sent to the new provider"
    );
    assert!(shell.ime_cursor_areas.lock().unwrap().is_empty());

    // The cursor area is reported to the new provider once layout has run
    doc.resolve(0.0);
    let expected = doc.get_node(input).unwrap().ime_cursor_area().unwrap();
    assert_eq!(*shell.ime_cursor_areas.lock().unwrap(), vec![expected]);

    // ...and only once while it is unchanged
    doc.resolve(0.0);
    assert_eq!(shell.ime_cursor_areas.lock().unwrap().len(), 1);
}

#[test]
fn ime_state_is_resent_when_focussed_after_layout() {
    let mut doc = make_doc();
    doc.resolve(0.0);
    let input = doc.query_selector("#input").unwrap().unwrap();
    doc.set_focus_to(input);

    // The area was already reported to the dummy provider, but must be
    // reported again to the new one
    let shell = Arc::new(RecordingShell::default());
    doc.set_shell_provider(shell.clone());
    doc.resend_ime_state();
    assert_eq!(*shell.ime_enabled_calls.lock().unwrap(), vec![true]);
    let expected = doc.get_node(input).unwrap().ime_cursor_area().unwrap();
    assert_eq!(*shell.ime_cursor_areas.lock().unwrap(), vec![expected]);
}

#[test]
fn ime_state_is_not_resent_for_non_text_input() {
    let mut doc = make_doc();
    let button = doc.query_selector("#button").unwrap().unwrap();
    doc.set_focus_to(button);

    let shell = Arc::new(RecordingShell::default());
    doc.set_shell_provider(shell.clone());
    doc.resend_ime_state();
    doc.resolve(0.0);
    assert!(shell.ime_enabled_calls.lock().unwrap().is_empty());
    assert!(shell.ime_cursor_areas.lock().unwrap().is_empty());
}
