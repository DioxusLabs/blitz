//! A restyle that changes only `text-align` lays the text out again without building its boxes
//! again.

use blitz_dom::DocumentConfig;
use blitz_dom::text::InlineText as _;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn make_doc(css: &str, body: &str) -> HtmlDocument {
    let html = format!(
        r#"<!DOCTYPE html><html><head><style>
            body {{ margin: 0; color: black; font-size: 16px; }}
            #owner {{ width: 300px; text-align: left; }}
            #owner:hover {{ text-align: right; }}
            {css}
        </style></head><body>{body}</body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.set_incremental_layout(true);
    doc.resolve(0.0);
    doc
}

fn hover_owner(doc: &mut HtmlDocument) {
    let owner = doc.query_selector("#owner").unwrap().unwrap();
    let pos = doc.get_node(owner).unwrap().absolute_position(1.0, 1.0);
    assert!(doc.set_hover_to(pos.x, pos.y));
    doc.resolve(0.0);
}

#[test]
fn alignment_restyle_keeps_the_inline_layout() {
    let mut doc = make_doc("", r#"<div id="owner">left <span>or</span> right</div>"#);
    let owner = doc.query_selector("#owner").unwrap().unwrap();
    // The block's inline layout, and where its text starts, in CSS pixels.
    let read = |doc: &HtmlDocument| {
        let layout = doc
            .get_node(owner)
            .unwrap()
            .element_data()
            .unwrap()
            .inline_layout_data
            .as_ref()
            .unwrap();
        let mut left = f64::INFINITY;
        layout.for_each_selection_rect(0, layout.text_len(), |rect| left = left.min(rect.x0));
        let left = (left / f64::from(layout.scale())) as f32;
        (&**layout as *const _ as usize, left)
    };
    let (before, start) = read(&doc);
    assert_eq!(start, 0.0);
    hover_owner(&mut doc);
    let (after, moved) = read(&doc);
    assert_eq!(after, before);
    assert!(moved > 0.0);
}

/// A textarea's editor takes the alignment it inherits after a restyle that keeps its box.
#[cfg(all(feature = "winkin", not(feature = "parley")))]
#[test]
fn alignment_restyle_reaches_a_textarea() {
    let mut doc = make_doc(
        "#field { width: 250px; }",
        r#"<div id="owner"><textarea id="field">ab</textarea></div>"#,
    );
    let field = doc.query_selector("#field").unwrap().unwrap();
    let start = |doc: &HtmlDocument| {
        let node = doc.get_node(field).unwrap();
        let editor = &node
            .element_data()
            .unwrap()
            .text_input_data()
            .unwrap()
            .editor;
        let layout = editor.text_layout().layout().unwrap();
        layout
            .lines()
            .map(|line| line.metrics().left)
            .fold(f32::INFINITY, f32::min)
    };
    assert_eq!(start(&doc), 0.0);
    hover_owner(&mut doc);
    assert!(start(&doc) > 0.0);
}
