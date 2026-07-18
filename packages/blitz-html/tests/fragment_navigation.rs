//! Fragment navigation: resolving a URL fragment (the `#...` part) to an element
//! and scrolling the viewport to it.

use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            // A short window so that tall content is scrollable.
            viewport: Some(Viewport::new(800, 200, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            font_ctx: Some(FontContext::new()),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

const HTML: &str = r#"<html><body style="margin:0">
    <div style="height:1000px"></div>
    <div id="target" style="height:50px"></div>
    <a name="named"></a>
    <div style="height:1000px"></div>
</body></html>"#;

#[test]
fn get_fragment_target_matches_id() {
    let doc = layout_doc(HTML);
    let by_id = doc.query_selector("#target").unwrap().unwrap();
    assert_eq!(doc.get_fragment_target("target"), Some(by_id));
}

#[test]
fn get_fragment_target_matches_named_anchor() {
    let doc = layout_doc(HTML);
    let node = doc.get_fragment_target("named");
    assert!(node.is_some(), "named anchor should be found");
}

#[test]
fn get_fragment_target_returns_none_for_unknown() {
    let doc = layout_doc(HTML);
    assert_eq!(doc.get_fragment_target("does-not-exist"), None);
}

#[test]
fn scroll_to_fragment_scrolls_to_element() {
    let mut doc = layout_doc(HTML);

    let target = doc.query_selector("#target").unwrap().unwrap();
    let target_y = doc.get_node(target).unwrap().final_layout().location.y as f64;
    assert!(target_y > 0.0);

    let found = doc.scroll_to_fragment("target");
    assert!(found);
    assert_eq!(doc.viewport_scroll().y, target_y);
}

#[test]
fn scroll_to_fragment_top_scrolls_to_top() {
    let mut doc = layout_doc(HTML);

    // First scroll down to the target...
    doc.scroll_to_fragment("target");
    assert!(doc.viewport_scroll().y > 0.0);

    // ...then an empty fragment should return us to the top of the document.
    let found = doc.scroll_to_fragment("");
    assert!(found);
    assert_eq!(doc.viewport_scroll().y, 0.0);
}

#[test]
fn scroll_to_fragment_unknown_returns_false() {
    let mut doc = layout_doc(HTML);
    assert!(!doc.scroll_to_fragment("does-not-exist"));
    assert_eq!(doc.viewport_scroll().y, 0.0);
}
