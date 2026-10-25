//! Single-line text inputs are aligned in their line by the baseline of their text, rather than
//! by their bottom margin edge.
//!
//! NOTE: These tests measure real text so they require a usable font. Without
//! the `system-fonts` feature (enabled by default when testing the whole
//! workspace) text measures 0x0 and the assertions pass vacuously.

use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            font_ctx: Some(FontContext::new()),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn rect(doc: &HtmlDocument, selector: &str) -> (f32, f32) {
    let id = doc.query_selector(selector).unwrap().unwrap();
    let layout = doc.get_node(id).unwrap().final_layout();
    (layout.location.y, layout.size.height)
}

#[test]
fn text_input_aligns_by_text_baseline() {
    let doc = layout_doc(
        r#"<style>
            body { margin: 0; }
            span, input { font: 20px/20px sans-serif; border: 0; padding: 0; }
            span { display: inline-block; }
        </style>
        <div><span id="a">X</span><input id="b" style="padding-top: 30px"></div>"#,
    );
    let (a_y, _) = rect(&doc, "#a");
    let (b_y, _) = rect(&doc, "#b");
    assert_eq!(b_y + 30.0, a_y);
}

#[test]
// Regression test for TodoMVC, where the strut of the line containing the input (whose font is
// smaller than the input's) extended the line below the input, opening a gap beneath it.
fn padded_text_input_does_not_extend_its_line() {
    let doc = layout_doc(
        r#"<style>
            body { margin: 0; font: 14px/1.4em sans-serif; }
            input { width: 100%; font-size: 24px; line-height: 1.4em; padding: 16px; border: none; box-sizing: border-box; }
        </style>
        <header id="header"><input id="input" type="text"></header>"#,
    );
    let (_, header_height) = rect(&doc, "#header");
    let (_, input_height) = rect(&doc, "#input");
    assert_eq!(header_height, input_height);
}
