//! Scrollable overflow is stored in device pixels, so a change of scale factor
//! must recompute it for every node, not only for the inline formatting
//! contexts whose text it reshapes. Otherwise the painter culls content of
//! text-free boxes by their overflow at the previous scale.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

/// A zero-height box holding a 600px child, resolved at scale 1, then moved
/// to scale 1.5.
fn rescaled_doc() -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        r#"<html><body style="margin:0">
        <div id="outer" style="height:0"><div style="height:600px"></div></div>
        </body></html>"#,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc.set_viewport(Viewport::new(1200, 900, 1.5, ColorScheme::Light));
    doc
}

fn outer_overflow_height(doc: &HtmlDocument) -> f64 {
    let id = doc.query_selector("#outer").unwrap().unwrap();
    doc.get_node(id).unwrap().scrollable_overflow().height()
}

#[test]
fn scale_change_rescales_overflow_of_text_free_boxes() {
    let mut doc = rescaled_doc();
    doc.resolve(0.0);
    assert_eq!(outer_overflow_height(&doc), 900.0);
}

/// Reading the stylist device applies the pending scale change before the
/// next resolve, which must still recompute the overflow.
#[test]
fn scale_change_applied_before_resolve_rescales_overflow() {
    let mut doc = rescaled_doc();
    doc.stylist_device();
    doc.resolve(0.0);
    assert_eq!(outer_overflow_height(&doc), 900.0);
}
