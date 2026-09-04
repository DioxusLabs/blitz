//! A viewport resize should not restyle the whole document: styles are only
//! invalidated for origins whose media query results changed and for elements
//! whose styles use viewport units (vw/vh/etc).

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; }
    #static { width: 100px; height: 20px; }
    #vw { width: 50vw; height: 10vh; }
    #mq { width: 10px; }
    @media (min-width: 850px) {
        #mq { width: 30px; }
    }
</style></head>
<body><div id="static"></div><div id="vw"></div><div id="mq"></div></body></html>
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

fn used_width(doc: &HtmlDocument, selector: &str) -> f32 {
    let id = doc.query_selector(selector).unwrap().unwrap();
    doc.get_node(id).unwrap().final_layout().size.width
}

fn style_ptr(doc: &HtmlDocument, selector: &str) -> *const () {
    let id = doc.query_selector(selector).unwrap().unwrap();
    let node = doc.get_node(id).unwrap();
    let styles = node.primary_styles().unwrap();
    std::ptr::from_ref(&**styles).cast()
}

#[test]
fn viewport_units_update_after_resize() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    assert_eq!(used_width(&doc, "#vw"), 400.0);

    doc.viewport_mut().window_size = (700, 600);
    doc.resolve(0.0);

    assert_eq!(used_width(&doc, "#vw"), 350.0);
}

#[test]
fn media_query_flip_updates_styles_after_resize() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    assert_eq!(used_width(&doc, "#mq"), 10.0);

    doc.viewport_mut().window_size = (900, 600);
    doc.resolve(0.0);

    assert_eq!(used_width(&doc, "#mq"), 30.0);

    doc.viewport_mut().window_size = (800, 600);
    doc.resolve(0.0);

    assert_eq!(used_width(&doc, "#mq"), 10.0);
}

#[test]
fn resize_does_not_restyle_viewport_independent_elements() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    let static_style = style_ptr(&doc, "#static");
    let vw_style = style_ptr(&doc, "#vw");

    // Resize without crossing the media query breakpoint.
    doc.viewport_mut().window_size = (820, 600);
    doc.resolve(0.0);

    // The viewport-unit-using element was restyled...
    assert_eq!(used_width(&doc, "#vw"), 410.0);
    assert_ne!(style_ptr(&doc, "#vw"), vw_style);
    // ...but the viewport-independent element kept its computed style.
    assert_eq!(style_ptr(&doc, "#static"), static_style);
}
