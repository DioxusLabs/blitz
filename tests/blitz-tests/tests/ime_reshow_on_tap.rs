//! On a phone, dismissing the soft keyboard (e.g. with Back) leaves the text
//! input focused. Tapping that input again must bring the keyboard back, the
//! way it does in a browser. The IME is only enabled when focus changes, and
//! winit treats a second enable as a no-op (`AlreadyEnabled`), so a tap on the
//! already-focused input has to re-request it.

use blitz_dom::{Document, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_test_harness::pointer_event;
use blitz_traits::{
    events::{BlitzPointerId, MouseEventButton, MouseEventButtons, UiEvent},
    shell::{ColorScheme, ShellProvider, Viewport},
};
use keyboard_types::Modifiers;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct RecordingShell {
    ime: Mutex<Vec<bool>>,
}
impl ShellProvider for RecordingShell {
    fn set_ime_enabled(&self, is_enabled: bool) {
        self.ime.lock().unwrap().push(is_enabled);
    }
}
impl RecordingShell {
    fn take_ime(&self) -> Vec<bool> {
        std::mem::take(&mut self.ime.lock().unwrap())
    }
}

fn make_doc() -> (HtmlDocument, Arc<RecordingShell>) {
    let shell = Arc::new(RecordingShell::default());
    let mut doc = HtmlDocument::from_html(
        r#"<html><body style="margin:0">
            <input id="input" style="width:200px; height:30px">
        </body></html>"#,
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

fn tap(doc: &mut HtmlDocument, id: BlitzPointerId) {
    let down = pointer_event(
        id,
        20.0,
        15.0,
        MouseEventButton::Main,
        MouseEventButtons::from(MouseEventButton::Main),
        Modifiers::default(),
    );
    let up = pointer_event(
        id,
        20.0,
        15.0,
        MouseEventButton::Main,
        MouseEventButtons::None,
        Modifiers::default(),
    );
    doc.handle_ui_event(UiEvent::PointerDown(down));
    doc.handle_ui_event(UiEvent::PointerUp(up));
}

#[test]
fn tapping_the_focused_input_shows_the_keyboard_again() {
    let (mut doc, shell) = make_doc();

    tap(&mut doc, BlitzPointerId::Finger(0));
    assert_eq!(shell.take_ime(), vec![true]);

    // The user dismisses the keyboard; the input keeps focus. Tapping it
    // again must re-request the IME rather than repeat an ignored enable.
    tap(&mut doc, BlitzPointerId::Finger(1));
    assert_eq!(shell.take_ime(), vec![false, true]);
}

#[test]
fn clicking_the_focused_input_with_a_mouse_leaves_the_ime_alone() {
    let (mut doc, shell) = make_doc();

    tap(&mut doc, BlitzPointerId::Mouse);
    assert_eq!(shell.take_ime(), vec![true]);

    // Desktop IMEs don't hide on their own; restarting one would drop an
    // in-progress composition.
    tap(&mut doc, BlitzPointerId::Mouse);
    assert_eq!(shell.take_ime(), Vec::<bool>::new());
}
