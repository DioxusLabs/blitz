//! A link in an `<iframe>` whose `target` leaves the frame (`_blank`, `_top`,
//! `_parent` in a frame of the top-level document, or any other name) navigates
//! through the top-level document's navigation provider. A link without a target,
//! or with `_self`, loads into the frame.

use blitz_dom::{Document, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point,
        PointerCoords, PointerDetails, UiEvent,
    },
    navigation::{NavigationOptions, NavigationProvider},
    net::{NetHandler, NetProvider, Request},
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

/// Records the URLs fetched, as when a frame loads a link.
#[derive(Default)]
struct Fetches(Mutex<Vec<String>>);

impl NetProvider for Fetches {
    fn fetch(&self, _doc_id: usize, request: Request, _handler: Box<dyn NetHandler>) {
        self.0.lock().unwrap().push(request.url.to_string());
    }
}

/// Where a click on a framed link went.
struct Navigated {
    /// What the top-level document navigated to.
    top: Vec<String>,
    /// What was fetched, which is where the frame navigated.
    fetched: Vec<String>,
}

/// Clicks a link that fills an iframe at the top left of the page.
fn click_framed_link(target: &str) -> Navigated {
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
    let fetches = Arc::new(Fetches::default());
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(200, 200, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            navigation_provider: Some(recorder.clone()),
            net_provider: Some(fetches.clone()),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    let event = pointer_event(50.0, 50.0);
    doc.handle_ui_event(UiEvent::PointerDown(event.clone()));
    doc.handle_ui_event(UiEvent::PointerUp(event));
    doc.handle_messages();

    Navigated {
        top: recorder.0.lock().unwrap().clone(),
        fetched: fetches.0.lock().unwrap().clone(),
    }
}

const URL: &str = "https://example.com/";

fn assert_leaves_frame(target: &str) {
    let navigated = click_framed_link(target);
    assert_eq!(navigated.top, [URL], "{target}");
    assert!(navigated.fetched.is_empty(), "{target}");
}

fn assert_stays_in_frame(target: &str) {
    let navigated = click_framed_link(target);
    assert!(navigated.top.is_empty(), "{target}");
    assert_eq!(navigated.fetched, [URL], "{target}");
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
fn blank_top_and_parent_leave_the_frame() {
    assert_leaves_frame("target='_blank'");
    assert_leaves_frame("target='_top'");
    assert_leaves_frame("target='_parent'");
    assert_leaves_frame("target='_BLANK'");
}

#[test]
fn a_named_target_leaves_the_frame() {
    assert_leaves_frame("target='_new'");
    assert_leaves_frame("target='article'");
}

#[test]
fn no_target_or_self_stays_in_the_frame() {
    assert_stays_in_frame("");
    assert_stays_in_frame("target='_self'");
}
