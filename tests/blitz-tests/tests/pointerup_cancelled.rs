//! A script that cancels `pointerup` stops its effects, but the document still sees the
//! pointer lift: the drag under way ends and the click is dispatched.

use blitz_dom::{Document, DocumentConfig, EventDriver, EventHandler, NodeId, ScrollBehavior};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_test_harness::pointer_event;
use blitz_traits::events::{
    BlitzPointerId, DomEvent, DomEventData, EventState, MouseEventButton, MouseEventButtons,
    UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use keyboard_types::Modifiers;

use std::sync::Arc;

/// Cancels every `pointerup` and records the names of the events it sees.
#[derive(Default)]
struct CancelPointerUp {
    events: Vec<String>,
}

impl EventHandler for &mut CancelPointerUp {
    fn handle_event(
        &mut self,
        _chain: &[NodeId],
        event: &mut DomEvent,
        _doc: &mut dyn Document,
        event_state: &mut EventState,
    ) {
        self.events.push(event.name().to_string());
        if matches!(event.data, DomEventData::PointerUp(_)) {
            event_state.prevent_default();
        }
    }
}

/// Send mouse events as `(y, button held)` steps at x = 97, over the scroller's scrollbar.
fn mouse(doc: &mut HtmlDocument, handler: &mut CancelPointerUp, steps: &[(f32, bool, u8)]) {
    let mut driver = EventDriver::new(doc, handler);
    for &(y, held, kind) in steps {
        let buttons = if held {
            MouseEventButtons::Primary
        } else {
            MouseEventButtons::None
        };
        let event = pointer_event(
            BlitzPointerId::Mouse,
            97.0,
            y,
            MouseEventButton::Main,
            buttons,
            Modifiers::default(),
        );
        driver.handle_ui_event(match kind {
            0 => UiEvent::PointerDown(event),
            1 => UiEvent::PointerMove(event),
            _ => UiEvent::PointerUp(event),
        });
    }
}

const DOWN: u8 = 0;
const MOVE: u8 = 1;
const UP: u8 = 2;

fn scroller_doc() -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        r#"<html><body style="margin:0">
            <div id="scroller" style="width:100px; height:100px; overflow-y:auto;">
                <div style="height:1000px;"></div>
            </div>
        </body></html>"#,
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

#[test]
fn a_cancelled_pointerup_still_clicks() {
    let mut doc = scroller_doc();
    let mut handler = CancelPointerUp::default();
    mouse(
        &mut doc,
        &mut handler,
        &[(50.0, true, DOWN), (50.0, false, UP)],
    );
    assert!(handler.events.iter().any(|name| name == "click"));
}

#[test]
fn a_cancelled_pointerup_still_ends_a_scrollbar_drag() {
    let mut doc = scroller_doc();
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();
    // Scrolling shows the scrollbar, whose thumb then spans y in [36, 68].
    doc.scroll_by(scroller, 0.0, 450.0, ScrollBehavior::Instant);

    let mut handler = CancelPointerUp::default();
    mouse(
        &mut doc,
        &mut handler,
        &[(50.0, true, DOWN), (50.0, false, UP), (16.0, false, MOVE)],
    );
    assert_eq!(doc.get_node(scroller).unwrap().scroll_offset().y, 450.0);
}
