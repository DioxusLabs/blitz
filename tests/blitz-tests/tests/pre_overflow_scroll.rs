//! A wide single-line `<pre>` inside a narrower `overflow-x: auto` container
//! must contribute its full unwrapped line width to the container's
//! scrollable overflow so the container can scroll horizontally.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

#[test]
fn wide_pre_makes_scroller_scrollable() {
    let html = r#"<html><body style="margin:0">
        <div id="scroller" style="width: 500px; overflow-x: auto;">
          <pre id="pre">one very long single line of text that is much wider than five hundred pixels and should cause horizontal scrolling inside the overflow-x auto container</pre>
        </div>
    </body></html>"#;

    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    let pre_id = doc.query_selector("#pre").unwrap().expect("#pre");
    let pre_layout = doc.get_node(pre_id).unwrap().final_layout();
    assert!(
        pre_layout.content_size.width > pre_layout.size.width,
        "expected the pre's content to overflow its border box, got size={:?} content_size={:?}",
        pre_layout.size,
        pre_layout.content_size
    );

    let scroller_id = doc.query_selector("#scroller").unwrap().expect("#scroller");
    let layout = doc.get_node(scroller_id).unwrap().final_layout();
    assert!(
        layout.scroll_width() > 100.0,
        "expected horizontal scrollable overflow, got scroll_width={}",
        layout.scroll_width()
    );
}
