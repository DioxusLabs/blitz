//! A comment node between block-level content must not affect layout.
//!
//! Regression test: a container holding [comment, block/abspos children] was
//! classified as an inline root (the comment defaulted to display:inline and
//! the abspos child was skipped as out-of-flow, leaving all_inline == true),
//! which swallowed element children into a parley inline layout as zero-sized
//! out-of-flow boxes.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout_doc(html: &str) -> HtmlDocument {
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

#[test]
fn comment_sibling_does_not_zero_abspos_child() {
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:300px; height:200px;">
                <!-- dioxus placeholder -->
                <div id="abs" style="position:absolute; top:0; left:0; right:0; bottom:0;"></div>
            </div>
        </body></html>"#,
    );
    let abs_id = doc.query_selector("#abs").unwrap().expect("#abs not found");
    let layout = doc.get_node(abs_id).unwrap().final_layout;
    assert_eq!(
        (layout.size.width, layout.size.height),
        (300.0, 200.0),
        "absolute inset:0 child must stretch to its containing block even \
         with a comment sibling"
    );
}

#[test]
fn comment_sibling_does_not_inline_block_child() {
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div style="width:300px;">
                <!-- dioxus placeholder -->
                <div id="block" style="height:50px;"></div>
            </div>
        </body></html>"#,
    );
    let block_id = doc
        .query_selector("#block")
        .unwrap()
        .expect("#block not found");
    let layout = doc.get_node(block_id).unwrap().final_layout;
    assert_eq!(
        (layout.size.width, layout.size.height),
        (300.0, 50.0),
        "block child must fill its parent's width even with a comment sibling"
    );
}
