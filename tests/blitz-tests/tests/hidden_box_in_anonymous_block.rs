//! A `display: none` box between two runs of inline content in a block that
//! also has block children must not split the runs into two anonymous blocks
//! (and so onto two lines).
//!
//! NOTE: measures real text, so it needs a usable font (`system-fonts`).

use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn height_of_container(inner: &str) -> f32 {
    let html = format!(
        r#"<html><body style="margin:0; font-size:16px">
            <div id="c" style="width:400px"><div>block child</div>{inner}</div>
        </body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            font_ctx: Some(FontContext::new()),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let c = doc.query_selector("#c").unwrap().unwrap();
    doc.get_node(c).unwrap().final_layout().size.height
}

#[test]
fn display_none_between_text_runs_keeps_one_line() {
    let without = height_of_container("first words second words");
    let with =
        height_of_container(r#"first words <span style="display:none">x</span>second words"#);
    assert_eq!(with, without);
}

#[test]
fn display_none_element_between_text_runs_keeps_one_line() {
    let without = height_of_container("first words second words");
    let with = height_of_container(r#"first words <style>p { color: red }</style>second words"#);
    assert_eq!(with, without);
}
