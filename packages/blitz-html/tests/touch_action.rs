//! The `touch-action` CSS property controls whether touch (finger) input may pan/scroll an
//! element. `none` blocks panning on both axes, `pan-x`/`pan-y` restrict it to a single axis, and
//! `auto`/`manipulation` permit panning on both axes. Mouse input is unaffected.

use blitz_dom::{Document, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point,
        PointerCoords, PointerDetails, UiEvent,
    },
    shell::{ColorScheme, Viewport},
};
use std::sync::Arc;

fn doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(200, 200, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn finger_event(x: f32, y: f32) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Finger(0),
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

/// Perform a finger pan gesture from `(x0, y0)` moving by `(dx, dy)` and return the resulting
/// scroll offset of the `#scroller` element.
///
/// The gesture uses three moves: the first crosses the drag threshold and starts the pan (its
/// delta is not applied), and the following moves produce the actual scroll delta.
fn pan(doc: &mut HtmlDocument, dx: f32, dy: f32) -> (f64, f64) {
    let (x0, y0) = (100.0f32, 100.0f32);
    doc.handle_ui_event(UiEvent::PointerDown(finger_event(x0, y0)));
    // Cross the 2px threshold to begin panning (this move applies no scroll delta).
    doc.handle_ui_event(UiEvent::PointerMove(finger_event(x0 + dx, y0 + dy)));
    // Move back to the origin, producing a scroll delta of (dx, dy).
    doc.handle_ui_event(UiEvent::PointerMove(finger_event(x0, y0)));
    doc.handle_ui_event(UiEvent::PointerUp(finger_event(x0, y0)));

    let scroller = doc.query_selector("#scroller").unwrap().expect("#scroller");
    let offset = doc.get_node(scroller).unwrap().scroll_offset;
    (offset.x, offset.y)
}

fn scroller_doc(touch_action: &str) -> HtmlDocument {
    doc(&format!(
        r#"<html><body style="margin:0">
            <div id="scroller" style="overflow:scroll; width:200px; height:200px; touch-action:{touch_action};">
                <div style="width:400px; height:400px;"></div>
            </div>
        </body></html>"#
    ))
}

#[test]
fn auto_allows_panning_on_both_axes() {
    let mut doc = scroller_doc("auto");
    let (x, _) = pan(&mut doc, 60.0, 0.0);
    assert!(x > 0.0, "touch-action:auto should allow horizontal panning");

    let mut doc = scroller_doc("auto");
    let (_, y) = pan(&mut doc, 0.0, 60.0);
    assert!(y > 0.0, "touch-action:auto should allow vertical panning");
}

#[test]
fn none_blocks_panning() {
    let mut doc = scroller_doc("none");
    let (x, y) = pan(&mut doc, 60.0, 0.0);
    assert_eq!(
        (x, y),
        (0.0, 0.0),
        "touch-action:none should block horizontal panning"
    );

    let mut doc = scroller_doc("none");
    let (x, y) = pan(&mut doc, 0.0, 60.0);
    assert_eq!(
        (x, y),
        (0.0, 0.0),
        "touch-action:none should block vertical panning"
    );
}

#[test]
fn pan_x_allows_only_horizontal() {
    let mut doc = scroller_doc("pan-x");
    let (x, y) = pan(&mut doc, 60.0, 0.0);
    assert!(
        x > 0.0,
        "touch-action:pan-x should allow horizontal panning"
    );
    assert_eq!(y, 0.0, "horizontal pan must not scroll vertically");

    let mut doc = scroller_doc("pan-x");
    let (_, y) = pan(&mut doc, 0.0, 60.0);
    assert_eq!(y, 0.0, "touch-action:pan-x should block vertical panning");
}

#[test]
fn pan_y_allows_only_vertical() {
    let mut doc = scroller_doc("pan-y");
    let (x, y) = pan(&mut doc, 0.0, 60.0);
    assert!(y > 0.0, "touch-action:pan-y should allow vertical panning");
    assert_eq!(x, 0.0, "vertical pan must not scroll horizontally");

    let mut doc = scroller_doc("pan-y");
    let (x, _) = pan(&mut doc, 60.0, 0.0);
    assert_eq!(x, 0.0, "touch-action:pan-y should block horizontal panning");
}

#[test]
fn manipulation_allows_panning_on_both_axes() {
    let mut doc = scroller_doc("manipulation");
    let (x, _) = pan(&mut doc, 60.0, 0.0);
    assert!(
        x > 0.0,
        "touch-action:manipulation should allow horizontal panning"
    );

    let mut doc = scroller_doc("manipulation");
    let (_, y) = pan(&mut doc, 0.0, 60.0);
    assert!(
        y > 0.0,
        "touch-action:manipulation should allow vertical panning"
    );
}
