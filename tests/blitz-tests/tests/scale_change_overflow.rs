//! Scrollable overflow is stored in device pixels, so a change of scale factor
//! must recompute it for every node, not only for the inline formatting
//! contexts whose text it reshapes. Otherwise the painter culls content of
//! text-free boxes by their overflow at the previous scale.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

#[test]
fn scale_change_rescales_overflow_of_text_free_boxes() {
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
    doc.resolve(0.0);

    let id = doc.query_selector("#outer").unwrap().unwrap();
    let overflow = *doc.get_node(id).unwrap().scrollable_overflow();
    assert_eq!(overflow.height(), 900.0);
}
