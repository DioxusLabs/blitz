//! A table is never shorter than its rows (CSS 2.1 §17.5.3): its `height` is
//! a minimum, and `max-height` doesn't make it shorter than its content.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn table_height(table_style: &str) -> f32 {
    let html = format!(
        r#"<html><body style="margin:0">
            <table id="t" style="{table_style}" cellspacing="0" cellpadding="0"><tr><td style="height:80px"></td></tr></table>
        </body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let t = doc.query_selector("#t").unwrap().unwrap();
    doc.get_node(t).unwrap().final_layout().size.height
}

#[test]
fn height_is_a_minimum() {
    assert_eq!(table_height("height:30px"), 80.0);
    assert_eq!(table_height("height:120px"), 120.0);
}

#[test]
fn max_height_does_not_shrink_the_table() {
    assert_eq!(table_height("max-height:30px"), 80.0);
    assert_eq!(table_height("height:120px; max-height:100px"), 100.0);
}
