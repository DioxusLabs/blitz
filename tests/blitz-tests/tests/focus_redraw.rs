//! Moving the focus changes what is painted, because the focus is styled
//! (`:focus`). A keyboard focus change touches neither layout nor content, so
//! unless the focus change itself asks for a frame, nothing does, and the
//! previous focus styling stays on screen until an unrelated event brings a
//! redraw along.

use blitz_dom::{Document, DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{BlitzKeyEvent, KeyState, UiEvent},
    shell::{ColorScheme, ShellProvider, Viewport},
};
use keyboard_types::{Code, Key, Location, Modifiers};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct RecordingShell {
    redraws: AtomicUsize,
}
impl ShellProvider for RecordingShell {
    fn request_redraw(&self) {
        self.redraws.fetch_add(1, Ordering::Relaxed);
    }
}
impl RecordingShell {
    fn take_redraws(&self) -> usize {
        self.redraws.swap(0, Ordering::Relaxed)
    }
}

const HTML: &str = r#"
    <html><body>
        <input id="first" type="text">
        <input id="second" type="text">
    </body></html>
"#;

fn make_doc() -> (HtmlDocument, Arc<RecordingShell>) {
    let shell = Arc::new(RecordingShell::default());
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            shell_provider: Some(shell.clone() as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    (doc, shell)
}

fn node_id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().expect(selector)
}

fn tab_key_event() -> BlitzKeyEvent {
    BlitzKeyEvent {
        key: Key::Tab,
        code: Code::Tab,
        modifiers: Modifiers::empty(),
        location: Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state: KeyState::Pressed,
        text: None,
    }
}

#[test]
fn focusing_a_node_requests_a_redraw() {
    let (mut doc, shell) = make_doc();
    let first = node_id(&doc, "#first");
    shell.take_redraws();

    assert!(doc.set_focus_to(first));
    assert_eq!(shell.take_redraws(), 1);
}

#[test]
fn refocusing_the_same_node_requests_nothing() {
    let (mut doc, shell) = make_doc();
    let first = node_id(&doc, "#first");
    doc.set_focus_to(first);
    shell.take_redraws();

    assert!(!doc.set_focus_to(first));
    assert_eq!(shell.take_redraws(), 0);
}

#[test]
fn blurring_requests_a_redraw() {
    let (mut doc, shell) = make_doc();
    let first = node_id(&doc, "#first");
    doc.set_focus_to(first);
    shell.take_redraws();

    doc.clear_focus();
    assert_eq!(shell.take_redraws(), 1);

    // Nothing was focused, so nothing changed on screen.
    doc.clear_focus();
    assert_eq!(shell.take_redraws(), 0);
}

#[test]
fn tabbing_to_the_next_node_requests_a_redraw() {
    let (mut doc, shell) = make_doc();
    let first = node_id(&doc, "#first");
    doc.set_focus_to(first);
    shell.take_redraws();

    doc.handle_ui_event(UiEvent::KeyDown(tab_key_event()));

    assert_eq!(doc.get_focussed_node_id(), Some(node_id(&doc, "#second")));
    assert_eq!(shell.take_redraws(), 1);
}
