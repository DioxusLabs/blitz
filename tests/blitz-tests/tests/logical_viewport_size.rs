//! `Viewport::set_logical_size` lays a document out at an exact, fractional
//! CSS size, as a paged medium needs: the page area of an A4 page with 18mm
//! margins is 657.6px wide. Percentages, viewport units and media queries see
//! that size, not the window size rounded to whole pixels.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; }
    #percent { width: 100%; height: 10px; }
    #vw { width: 100vw; height: 10px; }
    #mq { width: 1px; height: 10px; }
    @media (min-width: 657.5px) { #mq { width: 2px; } }
    @media (min-width: 658px) { #mq { width: 3px; } }
</style></head>
<body><div id="percent"></div><div id="vw"></div><div id="mq"></div></body></html>
"#;

fn make_doc(viewport: Viewport) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(viewport),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

/// The unrounded width: `final_layout` is rounded to whole pixels for
/// painting, while a paginating embedder such as a PDF writer reads this one.
fn width(doc: &HtmlDocument, selector: &str) -> f32 {
    let id = doc.query_selector(selector).unwrap().unwrap();
    doc.get_node(id).unwrap().unrounded_layout().size.width
}

fn logical(width: f32, height: f32) -> Viewport {
    let mut viewport = Viewport::new(0, 0, 1.0, ColorScheme::Light);
    viewport.set_logical_size(width, height);
    viewport
}

#[test]
fn layout_uses_the_fractional_width() {
    let doc = make_doc(logical(657.6, 900.0));
    assert!(
        (width(&doc, "#percent") - 657.6).abs() < 0.01,
        "{}",
        width(&doc, "#percent")
    );
    assert!(
        (width(&doc, "#vw") - 657.6).abs() < 0.01,
        "{}",
        width(&doc, "#vw")
    );
    // 657.5 <= 657.6 < 658
    assert_eq!(width(&doc, "#mq"), 2.0);
}

#[test]
fn a_new_viewport_with_a_fractional_width_relayouts() {
    let mut doc = make_doc(Viewport::new(800, 600, 1.0, ColorScheme::Light));
    assert_eq!(width(&doc, "#percent"), 800.0);
    assert_eq!(width(&doc, "#mq"), 3.0);
    doc.set_viewport(logical(300.25, 600.0));
    doc.resolve(0.0);
    assert!(
        (width(&doc, "#percent") - 300.25).abs() < 0.01,
        "{}",
        width(&doc, "#percent")
    );
    assert_eq!(width(&doc, "#mq"), 1.0);
}

#[test]
fn the_window_size_is_rounded_up_and_scaled() {
    let mut viewport = Viewport::new(0, 0, 2.0, ColorScheme::Light);
    viewport.set_logical_size(657.6, 900.0);
    assert_eq!(viewport.window_size, (1316, 1800));
    assert_eq!(viewport.logical_size(), (657.6, 900.0));
    // At a scale of 2 the layout is still 657.6 CSS pixels wide.
    let doc = make_doc(viewport);
    assert!(
        (width(&doc, "#percent") - 657.6).abs() < 0.01,
        "{}",
        width(&doc, "#percent")
    );
}
