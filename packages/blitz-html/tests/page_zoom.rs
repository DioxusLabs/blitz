//! Page zoom via `zoom_by_factor_at` (e.g. Ctrl/Cmd+scroll) must apply the zoom factor
//! multiplicatively, clamp the resulting zoom level, and adjust the viewport scroll so
//! that the content under the anchor point remains stationary.

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; width: 4000px; height: 4000px; }
</style></head>
<body></body></html>
"#;

fn make_doc(width: u32, height: u32) -> HtmlDocument {
    HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(width, height, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    )
}

#[test]
fn zoom_by_factor_applies_multiplicatively() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    doc.zoom_by_factor_at(2.0, 0.0, 0.0);
    assert_eq!(doc.viewport().zoom(), 2.0);

    doc.zoom_by_factor_at(0.5, 0.0, 0.0);
    assert_eq!(doc.viewport().zoom(), 1.0);
}

#[test]
fn zoom_by_factor_clamps_zoom_level() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    doc.zoom_by_factor_at(1000.0, 0.0, 0.0);
    assert_eq!(doc.viewport().zoom(), BaseDocument::MAX_ZOOM);

    doc.zoom_by_factor_at(0.0001, 0.0, 0.0);
    assert_eq!(doc.viewport().zoom(), BaseDocument::MIN_ZOOM);

    // Invalid factors are ignored
    doc.zoom_by_factor_at(f32::NAN, 0.0, 0.0);
    doc.zoom_by_factor_at(-1.0, 0.0, 0.0);
    doc.zoom_by_factor_at(0.0, 0.0, 0.0);
    assert_eq!(doc.viewport().zoom(), BaseDocument::MIN_ZOOM);
}

#[test]
fn zoom_by_factor_keeps_anchor_point_stationary() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 100.0 });

    // Zoom in 2x anchored at the vertical centre of the viewport (y = 300).
    // The content point under the anchor is at y = 400 in CSS pixels.
    doc.zoom_by_factor_at(2.0, 0.0, 300.0);

    // At 2x zoom the anchor's viewport-relative position is y = 150 CSS pixels,
    // so the scroll offset must be y = 250 for the content point to stay put.
    let scroll = doc.viewport_scroll();
    assert_eq!(scroll.y, 250.0);
    assert_eq!(scroll.y + 300.0 / 2.0, 400.0);

    // Zooming back out re-derives the original scroll position. The anchor is
    // still the viewport centre, which at 2x zoom is y = 150 in CSS pixels.
    doc.zoom_by_factor_at(0.5, 0.0, 150.0);
    assert_eq!(doc.viewport_scroll().y, 100.0);
}
