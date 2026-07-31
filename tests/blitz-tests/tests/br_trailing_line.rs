//! A forced line break (`<br>`) at the end of a block's inline content ends
//! the final line box but must not generate an extra empty line box below it.
//!
//! Regression test: parley generates a trailing empty line for text ending in
//! a newline (text-editor semantics), which inflated the height of blocks
//! ending in `<br>` by one line. On Hacker News comment pages this manifested
//! as an unexpected vertical gap between a comment's header (`.comhead`,
//! wrapped in a `margin-bottom:-10px` div followed by `<br>`) and its body
//! (`.comment`).
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

#[test]
#[ignore = "Need to find better fix for this issue"]
fn negative_margin_bottom_pulls_up_sibling() {
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div id="head" style="margin-top:0px; margin-bottom:-10px; height:20px;"></div>
            <div id="comment" style="height:50px;"></div>
        </body></html>"#,
    );
    let head = doc.query_selector("#head").unwrap().unwrap();
    let comment = doc.query_selector("#comment").unwrap().unwrap();
    let head_layout = doc.get_node(head).unwrap().final_layout();
    let comment_layout = doc.get_node(comment).unwrap().final_layout();
    println!(
        "head: y={} h={}",
        head_layout.location.y, head_layout.size.height
    );
    println!("comment: y={}", comment_layout.location.y);
    assert_eq!(comment_layout.location.y, 10.0);
}

#[test]
#[ignore = "Need to find better fix for this issue"]
fn hn_comhead_structure() {
    // Mimics HN: td > [div(inline content, margin-bottom:-10px), br, div.comment]
    let doc = layout_doc(
        r#"<html><body style="margin:0; font-size:12px;">
            <table border="0"><tr><td class="default"><div id="head" style="margin-top:2px; margin-bottom:-10px;"><span class="comhead">user 1 hour ago</span></div><br>
            <div id="comment"><span>Comment text</span></div></td></tr></table>
        </body></html>"#,
    );
    let head = doc.query_selector("#head").unwrap().unwrap();
    let comment = doc.query_selector("#comment").unwrap().unwrap();
    let head_layout = doc.get_node(head).unwrap().final_layout();
    let comment_layout = doc.get_node(comment).unwrap().final_layout();
    println!(
        "head: y={} h={} margin={:?}",
        head_layout.location.y, head_layout.size.height, head_layout.margin
    );
    println!("comment: y={}", comment_layout.location.y);
    let gap = comment_layout.location.y - (head_layout.location.y + head_layout.size.height);
    println!("gap: {gap}");
    // gap should be br-line-height minus 10
    assert!(gap < 10.0, "gap too large: {gap}");
}

#[test]
#[ignore = "Need to find better fix for this issue"]
fn br_with_trailing_whitespace_is_single_line() {
    let doc = layout_doc(
        "<html><body style=\"margin:0; font-size:12px;\">\
            <div id=\"one-line\">x</div>\
            <div id=\"br-only\"><br></div>\
            <div id=\"br-ws\"><br>\n            </div>\
        </body></html>",
    );
    let h = |sel: &str| {
        let id = doc.query_selector(sel).unwrap().unwrap();
        doc.get_node(id).unwrap().unrounded_layout().size.height
    };
    let one_line = h("#one-line");
    println!(
        "one-line: {}, br-only: {}, br-ws: {}",
        one_line,
        h("#br-only"),
        h("#br-ws")
    );
    assert_eq!(
        h("#br-only"),
        one_line,
        "div with only <br> should be one line tall"
    );
    assert_eq!(
        h("#br-ws"),
        one_line,
        "div with <br> + collapsible whitespace should be one line tall"
    );
}
