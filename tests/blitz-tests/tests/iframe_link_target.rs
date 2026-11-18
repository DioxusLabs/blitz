//! A link in an `<iframe>` with `target="_blank"` or `target="_top"` navigates
//! through the top-level document's navigation provider, as browsers take such
//! links out of the frame. Other links stay in the frame.

use blitz_dom::{Document, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point,
        PointerCoords, PointerDetails, UiEvent,
    },
    navigation::{NavigationOptions, NavigationProvider},
    shell::{ColorScheme, Viewport},
};
use std::sync::{Arc, Mutex};

/// Records the URLs the top-level document is asked to navigate to.
#[derive(Default)]
struct Recorder(Mutex<Vec<String>>);

impl NavigationProvider for Recorder {
    fn navigate_to(&self, options: NavigationOptions) {
        self.0.lock().unwrap().push(options.url.to_string());
    }
}

/// Clicks a link that fills an iframe at the top left of the page, and returns
/// what the top-level document navigated to.
fn click_framed_link(target: &str) -> Vec<String> {
    let link = format!(
        "<a href='https://example.com/' {target} \
         style='display:block; width:100px; height:100px'>link</a>"
    );
    let html = format!(
        "<html><body style='margin:0'>\
         <iframe srcdoc=\"<body style='margin:0'>{link}</body>\" \
         style='border:0; width:100px; height:100px; display:block'></iframe>\
         </body></html>"
    );
    let recorder = Arc::new(Recorder::default());
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(200, 200, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            navigation_provider: Some(recorder.clone()),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    let event = pointer_event(50.0, 50.0);
    doc.handle_ui_event(UiEvent::PointerDown(event.clone()));
    doc.handle_ui_event(UiEvent::PointerUp(event));
    doc.resolve(0.0);

    recorder.0.lock().unwrap().clone()
}

fn pointer_event(x: f32, y: f32) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button: MouseEventButton::Main,
        buttons: MouseEventButtons::from(MouseEventButton::Main),
        mods: Default::default(),
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    }
}

#[test]
fn target_blank_leaves_the_frame() {
    assert_eq!(
        click_framed_link("target='_blank'"),
        ["https://example.com/"]
    );
}

#[test]
fn target_top_leaves_the_frame() {
    assert_eq!(click_framed_link("target='_top'"), ["https://example.com/"]);
}

#[test]
fn a_link_without_a_target_stays_in_the_frame() {
    assert!(click_framed_link("").is_empty());
}
