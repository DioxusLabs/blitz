//! A script that cancels `pointerdown` stops its effects, such as selecting text, but the
//! document still sees the press: it counts towards a double click.

use blitz_dom::{DocumentConfig, EventDriver, EventHandler, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_test_harness::mouse_pointer_event;
use blitz_traits::events::{DomEvent, DomEventData, EventState, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};

use std::sync::Arc;

/// Cancels every `pointerdown` and records the names of the events it sees.
#[derive(Default)]
struct CancelPointerDown {
    events: std::cell::RefCell<Vec<String>>,
}

impl EventHandler for &CancelPointerDown {
    fn handle_event(
        &self,
        _chain: &[NodeId],
        event: &mut DomEvent,
        _ctx: &mut blitz_dom::EventContext<'_>,
        event_state: &mut EventState,
    ) {
        self.events.borrow_mut().push(event.name().to_string());
        if matches!(event.data, DomEventData::PointerDown(_)) {
            event_state.prevent_default();
        }
    }
}

#[test]
fn a_cancelled_pointerdown_counts_towards_a_double_click_without_selecting() {
    let mut doc = HtmlDocument::from_html(
        r#"<html><body style="margin:0"><div style="font-size:40px">Some text</div></body></html>"#,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    let handler = CancelPointerDown::default();
    let mut driver = EventDriver::new(&mut doc, &handler);
    for _ in 0..2 {
        driver.handle_ui_event(UiEvent::PointerDown(mouse_pointer_event(50.0, 20.0)));
        driver.handle_ui_event(UiEvent::PointerUp(mouse_pointer_event(50.0, 20.0)));
    }

    assert!(
        handler
            .events
            .borrow()
            .iter()
            .any(|name| name == "dblclick")
    );
    assert!(!doc.has_text_selection());
}
