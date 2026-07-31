//! `rem` units must keep resolving against the root element's font-size after
//! the viewport changes (e.g. a window resize).
//!
//! Regression test for www.wikipedia.org rendering "zoomed in" after a resize:
//! rebuilding the stylist device reset its cached root font-size to the initial
//! value (16px), and since the root's font-size doesn't *change* during the
//! subsequent restyle, stylo never re-seeded it. Every `rem` in the document
//! then resolved against 16px instead of the root's actual font-size.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    html { font-size: 10px; }
    body { margin: 0; }
    h1 { font-size: 3.2rem; }
    #box { width: 20rem; height: 5rem; }
</style></head>
<body><h1>Title</h1><div id="box"></div></body></html>
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

fn h1_font_size(doc: &HtmlDocument) -> f32 {
    let h1_id = doc.query_selector("h1").unwrap().unwrap();
    let node = doc.get_node(h1_id).unwrap();
    let styles = node.primary_styles().unwrap();
    styles.get_font().font_size.computed_size.0.px()
}

fn box_size(doc: &HtmlDocument) -> (f32, f32) {
    let box_id = doc.query_selector("#box").unwrap().unwrap();
    let layout = &doc.get_node(box_id).unwrap().final_layout();
    (layout.size.width, layout.size.height)
}

#[test]
fn rem_units_stable_across_viewport_resize() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    assert_eq!(h1_font_size(&doc), 32.0);
    assert_eq!(box_size(&doc), (200.0, 50.0));

    // Simulate a window resize (the winit resize path goes through viewport_mut,
    // which rebuilds the stylist device on drop).
    doc.viewport_mut().window_size = (900, 600);
    doc.resolve(0.0);

    assert_eq!(h1_font_size(&doc), 32.0);
    assert_eq!(box_size(&doc), (200.0, 50.0));
}

#[test]
fn rem_units_stable_across_scale_change() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    // Simulate a hidpi scale factor change
    doc.viewport_mut().set_hidpi_scale(2.0);
    doc.resolve(0.0);

    assert_eq!(h1_font_size(&doc), 32.0);
    assert_eq!(box_size(&doc), (200.0, 50.0));
}
