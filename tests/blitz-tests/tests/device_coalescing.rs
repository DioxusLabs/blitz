//! Device changes (viewport resizes, zoom, color-scheme, media-type) are
//! coalesced: they accumulate on the document and are applied to the stylist
//! as a single device rebuild at the start of the next resolve.

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
fn multiple_resizes_between_resolves_apply_latest_size() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    // Several resizes before the next resolve: only the latest matters.
    doc.viewport_mut().window_size = (900, 600);
    doc.viewport_mut().window_size = (1000, 700);
    doc.viewport_mut().window_size = (700, 500);
    doc.resolve(0.0);

    assert_eq!(used_width(&doc, "#vw"), 350.0);
    // 700px is below the 850px breakpoint, so #mq must have its narrow width
    // even though intermediate sizes crossed the breakpoint.
    assert_eq!(used_width(&doc, "#mq"), 10.0);
}

#[test]
fn resize_and_zoom_coalesce() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    {
        let mut viewport = doc.viewport_mut();
        viewport.window_size = (800, 600);
        *viewport.zoom_mut() = 2.0;
    }
    doc.resolve(0.0);

    // CSS viewport is 400x300; 50vw = 200 CSS px.
    assert_eq!(used_width(&doc, "#vw"), 200.0);
    assert_eq!(used_width(&doc, "#mq"), 10.0);
}

#[test]
fn stylist_device_read_flushes_pending_changes() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    doc.viewport_mut().window_size = (700, 500);

    // Reading the stylist device before the next resolve must observe the
    // pending viewport change.
    let size = doc.stylist_device().au_viewport_size();
    assert_eq!(size.width.to_f32_px(), 700.0);
    assert_eq!(size.height.to_f32_px(), 500.0);
}

#[test]
fn color_scheme_change_recascades_styles() {
    let mut doc = make_doc(800, 600);
    doc.resolve(0.0);

    let static_style = style_ptr(&doc, "#static");

    doc.viewport_mut().color_scheme = ColorScheme::Dark;
    doc.resolve(0.0);

    // Color-scheme-dependent values (light-dark(), system colors) resolve at
    // cascade time without necessarily flipping a media query, so a
    // color-scheme change must recascade even viewport-independent elements.
    assert_ne!(style_ptr(&doc, "#static"), static_style);
    // Layout is unaffected.
    assert_eq!(used_width(&doc, "#static"), 100.0);
}
