//! Fixed table layout: column percentages adding up to more than 100% are
//! scaled down to share the table, as browsers do.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn cell_widths(widths: &[&str]) -> Vec<f32> {
    let cells: String = widths
        .iter()
        .enumerate()
        .map(|(i, w)| format!(r#"<td id="c{i}" style="width:{w}"></td>"#))
        .collect();
    let html = format!(
        r#"<html><body style="margin:0">
            <table width="600" cellspacing="0" cellpadding="0" style="table-layout:fixed"><tr>{cells}</tr></table>
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
    (0..widths.len())
        .map(|i| {
            let id = doc.query_selector(&format!("#c{i}")).unwrap().unwrap();
            doc.get_node(id).unwrap().final_layout().size.width
        })
        .collect()
}

#[test]
fn overfull_percentages_share_the_table() {
    assert_eq!(cell_widths(&["100%", "100%"]), vec![300.0, 300.0]);
    assert_eq!(cell_widths(&["150%", "50%"]), vec![450.0, 150.0]);
}

#[test]
fn percentages_under_100_are_unchanged() {
    assert_eq!(cell_widths(&["25%", "75%"]), vec![150.0, 450.0]);
}
