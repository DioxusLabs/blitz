//! A table cell's `height` is the least it may be (CSS 2.2 §17.5.3): the row is as tall as the
//! tallest of its cells' heights and the height their content needs.
//!
//! Regression test: mail buttons are often a 40px link in `<td height="21">`; the cell stayed
//! 21px and the next row was drawn over the bottom half of the button.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

/// The height of `#id` and the top of `#after` (relative to its parent, the body).
fn cell_and_after(cell: &str) -> (f32, f32) {
    let doc = layout(&format!(
        r#"<body style="margin:0"><table style="border-spacing:0"><tr>{cell}<td style="padding:0">x</td></tr></table><p id="after" style="margin:0">after</p></body>"#
    ));
    let size = |sel: &str| {
        let id = doc.query_selector(sel).unwrap().expect(sel);
        doc.get_node(id).unwrap().final_layout()
    };
    (size("#id").size.height, size("#after").location.y)
}

const BUTTON: &str = r#"<div style="height:40px;width:100px"></div>"#;

#[test]
fn content_taller_than_a_css_height_grows_the_cell() {
    let cell = format!(r#"<td id="id" style="padding:0;height:21px">{BUTTON}</td>"#);
    assert_eq!(cell_and_after(&cell), (40.0, 40.0));
}

#[test]
fn content_taller_than_the_height_attribute_grows_the_cell() {
    let cell = format!(r#"<td id="id" height="21" style="padding:0">{BUTTON}</td>"#);
    assert_eq!(cell_and_after(&cell), (40.0, 40.0));
}

#[test]
fn a_height_taller_than_the_content_is_kept() {
    let cell = r#"<td id="id" style="padding:0;height:60px"><div style="height:10px"></div></td>"#;
    assert_eq!(cell_and_after(cell), (60.0, 60.0));
}

#[test]
fn padding_adds_to_a_content_box_height() {
    let cell =
        r#"<td id="id" style="padding:5px;height:30px"><div style="height:10px"></div></td>"#;
    assert_eq!(cell_and_after(cell), (40.0, 40.0));
}
