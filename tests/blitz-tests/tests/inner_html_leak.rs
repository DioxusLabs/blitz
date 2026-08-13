//! Repeated `set_inner_html` calls must not leak parser fragment roots.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<html><body><div id="target"></div></body></html>"#;
const INNER_HTML: &str = r#"<span>one</span><span>two</span>"#;

fn assert_inner_html_does_not_leak(incremental: bool) {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.set_incremental_layout(incremental);

    let target_id = doc
        .query_selector("#target")
        .unwrap()
        .expect("#target not found");

    doc.mutate().set_inner_html(target_id, INNER_HTML);
    let first_len = doc.tree().len();

    let target = doc.get_node(target_id).expect("target was removed");
    assert_eq!(target.children.len(), 2);
    assert_eq!(target.text_content(), "onetwo");

    for _ in 0..20 {
        doc.mutate().set_inner_html(target_id, INNER_HTML);
    }

    assert_eq!(
        doc.tree().len(),
        first_len,
        "repeated set_inner_html leaked nodes with incremental layout {incremental}"
    );
}

#[test]
fn repeated_set_inner_html_does_not_leak() {
    assert_inner_html_does_not_leak(false);
    assert_inner_html_does_not_leak(true);
}
