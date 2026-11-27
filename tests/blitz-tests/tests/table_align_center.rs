//! `<table align=center>` is centred by auto inline margins (HTML rendering:
//! tables), matching `align` case-insensitively.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn table_x(align: &str) -> f32 {
    let html = format!(
        r#"<html><body style="margin:0">
            <table id="t" align="{align}" width="200"><tr><td style="height:10px"></td></tr></table>
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
    doc.get_node(t).unwrap().final_layout().location.x
}

#[test]
fn centered_table_is_centered() {
    assert_eq!(table_x("center"), 300.0);
    assert_eq!(table_x("CENTER"), 300.0);
}
