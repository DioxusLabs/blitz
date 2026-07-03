//! Pinch-to-zoom (`pinch_zoom_by`) is a render-level ("visual viewport") zoom: it must
//! not affect style/layout, must keep the content under the gesture's anchor point
//! stationary, must be pannable within (and beyond, via document scroll) the page,
//! must route to hovered subdocuments, and must be disableable per document.

use blitz_dom::{BaseDocument, Document, DocumentConfig, Point};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::events::{BlitzWheelDelta, BlitzWheelEvent, PointerCoords, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; width: 4000px; height: 4000px; }
</style></head>
<body><p>Hello, world!</p></body></html>
"#;

fn make_doc(html: &str, width: u32, height: u32) -> HtmlDocument {
    HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(width, height, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            pinch_zoom_enabled: Some(true),
            ..Default::default()
        },
    )
}

#[test]
fn pinch_zoom_is_disabled_by_default() {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    assert!(!doc.pinch_zoom_enabled());
    assert!(!doc.pinch_zoom_by(2.0, 0.0, 0.0));
    assert_eq!(doc.pinch_zoom().scale, 1.0);
}

#[test]
fn pinch_zoom_does_not_affect_layout() {
    let mut doc = make_doc(HTML, 800, 600);
    doc.resolve(0.0);
    let initial_root_size = doc.root_element().final_layout.size;

    doc.pinch_zoom_by(2.0, 400.0, 300.0);
    doc.resolve(0.0);

    assert_eq!(doc.pinch_zoom().scale, 2.0);
    assert_eq!(doc.viewport().zoom(), 1.0);
    assert_eq!(doc.root_element().final_layout.size, initial_root_size);
}

#[test]
fn pinch_zoom_clamps_scale() {
    let mut doc = make_doc(HTML, 800, 600);
    doc.resolve(0.0);

    doc.pinch_zoom_by(1000.0, 0.0, 0.0);
    assert_eq!(doc.pinch_zoom().scale, BaseDocument::MAX_PINCH_ZOOM);

    doc.pinch_zoom_by(0.0001, 0.0, 0.0);
    assert_eq!(doc.pinch_zoom().scale, 1.0);
    assert_eq!(doc.pinch_zoom().offset, Point::ZERO);

    // Invalid factors are ignored
    assert!(!doc.pinch_zoom_by(f64::NAN, 0.0, 0.0));
    assert!(!doc.pinch_zoom_by(-1.0, 0.0, 0.0));
    assert!(!doc.pinch_zoom_by(0.0, 0.0, 0.0));
    assert_eq!(doc.pinch_zoom().scale, 1.0);
}

#[test]
fn pinch_zoom_keeps_anchor_stationary() {
    let mut doc = make_doc(HTML, 800, 600);
    doc.resolve(0.0);

    // Zoom in 2x anchored at the viewport centre (400, 300). The content point under
    // the anchor is at (400, 300); at 2x it is displayed at 2x its offset from the
    // visual viewport origin, so the offset must become (200, 150) for it to stay put.
    doc.pinch_zoom_by(2.0, 400.0, 300.0);
    assert_eq!(doc.pinch_zoom().offset, Point { x: 200.0, y: 150.0 });

    // Zoom in another 2x anchored at the (visual) viewport centre, which is still over
    // the content point (400, 300): offset must become (300, 225).
    doc.pinch_zoom_by(2.0, 400.0, 300.0);
    assert_eq!(doc.pinch_zoom().scale, 4.0);
    assert_eq!(doc.pinch_zoom().offset, Point { x: 300.0, y: 225.0 });

    // Zooming all the way back out re-centres the (un-scrolled) view
    doc.pinch_zoom_by(0.25, 400.0, 300.0);
    assert_eq!(doc.pinch_zoom().scale, 1.0);
    assert_eq!(doc.pinch_zoom().offset, Point::ZERO);

    // Layout-space pointer coordinates are unaffected by zooming in and back out
    assert_eq!(doc.viewport_scroll(), Point::ZERO);
}

#[test]
fn pinch_zoom_offset_overflow_scrolls_document() {
    let mut doc = make_doc(HTML, 800, 600);
    doc.resolve(0.0);
    doc.set_viewport_scroll(Point { x: 0.0, y: 500.0 });

    // Zoom in 2x anchored at the bottom-left corner: the offset lands exactly at its
    // maximum value (600 * (1 - 1/2) = 300).
    doc.pinch_zoom_by(2.0, 0.0, 600.0);
    assert_eq!(doc.pinch_zoom().offset, Point { x: 0.0, y: 300.0 });
    assert_eq!(doc.viewport_scroll(), Point { x: 0.0, y: 500.0 });

    // Zoom back out anchored at the top-left corner: keeping the content under the
    // anchor stationary requires an offset of 300, but the maximum offset at 1x is 0,
    // so the overflow is converted into document scroll.
    doc.pinch_zoom_by(0.5, 0.0, 0.0);
    assert_eq!(doc.pinch_zoom().scale, 1.0);
    assert_eq!(doc.pinch_zoom().offset, Point::ZERO);
    assert_eq!(doc.viewport_scroll(), Point { x: 0.0, y: 800.0 });
}

#[test]
fn scrolling_pans_visual_viewport_before_document() {
    let mut doc = make_doc(HTML, 800, 600);
    doc.resolve(0.0);

    doc.pinch_zoom_by(2.0, 0.0, 0.0);
    assert_eq!(doc.pinch_zoom().offset, Point::ZERO);

    // Scrolling down by 100 pans the visual viewport within the layout viewport
    doc.scroll_viewport_by(0.0, -100.0);
    assert_eq!(doc.pinch_zoom().offset, Point { x: 0.0, y: 100.0 });
    assert_eq!(doc.viewport_scroll(), Point::ZERO);

    // Scrolling down by a further 400 exhausts the pan range (max offset 300) and
    // scrolls the document by the remaining 200
    doc.scroll_viewport_by(0.0, -400.0);
    assert_eq!(doc.pinch_zoom().offset, Point { x: 0.0, y: 300.0 });
    assert_eq!(doc.viewport_scroll(), Point { x: 0.0, y: 200.0 });
}

#[test]
fn wheel_scroll_pans_at_visual_speed() {
    let mut doc = make_doc(HTML, 800, 600);
    doc.resolve(0.0);
    doc.pinch_zoom_by(2.0, 0.0, 0.0);

    // A 200px wheel scroll pans the visual viewport by 100 CSS pixels at 2x zoom
    // (matching the visual movement of the zoomed content)
    doc.handle_ui_event(UiEvent::Wheel(BlitzWheelEvent {
        delta: BlitzWheelDelta::Pixels(0.0, -200.0),
        coords: PointerCoords {
            page_x: 0.0,
            page_y: 0.0,
            screen_x: 0.0,
            screen_y: 0.0,
            client_x: 0.0,
            client_y: 0.0,
        },
        buttons: Default::default(),
        mods: Default::default(),
    }));

    assert_eq!(doc.pinch_zoom().offset, Point { x: 0.0, y: 100.0 });
    assert_eq!(doc.viewport_scroll(), Point::ZERO);
}

#[test]
fn pinch_zoom_can_be_disabled() {
    let mut doc = make_doc(HTML, 800, 600);
    doc.resolve(0.0);

    doc.set_pinch_zoom_enabled(false);
    assert!(!doc.pinch_zoom_by(2.0, 0.0, 0.0));
    assert_eq!(doc.pinch_zoom().scale, 1.0);

    // Re-enabling allows zooming again; disabling mid-zoom resets the zoom
    doc.set_pinch_zoom_enabled(true);
    assert!(doc.pinch_zoom_by(2.0, 0.0, 0.0));
    assert_eq!(doc.pinch_zoom().scale, 2.0);
    doc.set_pinch_zoom_enabled(false);
    assert_eq!(doc.pinch_zoom().scale, 1.0);
    assert_eq!(doc.pinch_zoom().offset, Point::ZERO);
}

const PARENT_HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; width: 800px; height: 600px; }
    #frame { position: absolute; left: 100px; top: 100px; width: 400px; height: 300px; }
</style></head>
<body><div id="frame"></div></body></html>
"#;

fn make_doc_with_subdoc() -> (HtmlDocument, usize) {
    let mut doc = make_doc(PARENT_HTML, 800, 600);
    let frame_id = doc.query_selector("#frame").unwrap().unwrap();
    let sub_doc = make_doc(HTML, 400, 300);
    doc.set_sub_document(frame_id, Box::new(sub_doc));
    doc.resolve(0.0);
    (doc, frame_id)
}

fn sub_doc_pinch_zoom(doc: &HtmlDocument, frame_id: usize) -> blitz_dom::PinchZoomState {
    doc.get_node(frame_id)
        .unwrap()
        .subdoc()
        .unwrap()
        .inner()
        .pinch_zoom()
}

#[test]
fn pinch_zoom_routes_to_hovered_subdocument() {
    let (mut doc, frame_id) = make_doc_with_subdoc();

    // Hovering the subdocument routes the zoom to it (anchored relative to its origin)
    doc.set_hover_to(300.0, 200.0);
    assert!(doc.pinch_zoom_by(2.0, 300.0, 200.0));
    assert_eq!(doc.pinch_zoom().scale, 1.0);
    let sub_state = sub_doc_pinch_zoom(&doc, frame_id);
    assert_eq!(sub_state.scale, 2.0);
    assert_eq!(sub_state.offset, Point { x: 100.0, y: 50.0 });

    // Hovering outside of the subdocument zooms the document itself
    doc.set_hover_to(700.0, 50.0);
    assert!(doc.pinch_zoom_by(2.0, 700.0, 50.0));
    assert_eq!(doc.pinch_zoom().scale, 2.0);
    assert_eq!(sub_doc_pinch_zoom(&doc, frame_id).scale, 2.0);
}

#[test]
fn disabled_subdocument_passes_pinch_zoom_to_parent() {
    let (mut doc, frame_id) = make_doc_with_subdoc();
    doc.subdoc_mut(frame_id)
        .unwrap()
        .inner_mut()
        .set_pinch_zoom_enabled(false);

    doc.set_hover_to(300.0, 200.0);
    assert!(doc.pinch_zoom_by(2.0, 300.0, 200.0));
    assert_eq!(doc.pinch_zoom().scale, 2.0);
    assert_eq!(sub_doc_pinch_zoom(&doc, frame_id).scale, 1.0);
}

fn inline_layout_scale(doc: &blitz_dom::BaseDocument, selector: &str) -> f32 {
    let node_id = doc.query_selector(selector).unwrap().unwrap();
    doc.get_node(node_id)
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .unwrap()
        .layout
        .scale()
}

#[test]
fn pinch_zoom_scales_text_layouts() {
    let mut doc = make_doc(HTML, 800, 600);
    doc.resolve(0.0);
    assert_eq!(doc.text_layout_scale(), 1.0);
    assert_eq!(inline_layout_scale(&doc, "p"), 1.0);

    // Pinch-zooming invalidates inline layouts, which are rebuilt at the zoomed scale
    doc.pinch_zoom_by(2.0, 0.0, 0.0);
    doc.resolve(0.0);
    assert_eq!(doc.text_layout_scale(), 2.0);
    assert_eq!(inline_layout_scale(&doc, "p"), 2.0);

    doc.reset_pinch_zoom();
    doc.resolve(0.0);
    assert_eq!(inline_layout_scale(&doc, "p"), 1.0);
}

#[test]
fn parent_pinch_zoom_scales_subdocument_text_layouts() {
    let (mut doc, frame_id) = make_doc_with_subdoc();

    // Zoom the parent document (pointer not over the subdocument)
    doc.set_hover_to(700.0, 50.0);
    assert!(doc.pinch_zoom_by(2.0, 700.0, 50.0));
    doc.resolve(0.0);

    // The subdocument's text layouts are computed at the accumulated scale
    let sub_doc = doc.get_node(frame_id).unwrap().subdoc().unwrap().inner();
    assert_eq!(sub_doc.pinch_zoom().scale, 1.0);
    assert_eq!(sub_doc.text_layout_scale(), 2.0);
    assert_eq!(inline_layout_scale(&sub_doc, "p"), 2.0);
}

#[test]
fn disabled_document_still_zooms_hovered_subdocument() {
    let (mut doc, frame_id) = make_doc_with_subdoc();
    doc.set_pinch_zoom_enabled(false);

    doc.set_hover_to(300.0, 200.0);
    assert!(doc.pinch_zoom_by(2.0, 300.0, 200.0));
    assert_eq!(doc.pinch_zoom().scale, 1.0);
    assert_eq!(sub_doc_pinch_zoom(&doc, frame_id).scale, 2.0);

    // But not when the pointer is outside of the subdocument
    doc.set_hover_to(700.0, 50.0);
    assert!(!doc.pinch_zoom_by(2.0, 700.0, 50.0));
    assert_eq!(doc.pinch_zoom().scale, 1.0);
}
