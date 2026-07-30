//! `BaseDocument::node_client_rects` must return one rect per line-box
//! fragment for non-atomic inline elements (which have no Taffy layout box of
//! their own), and `get_client_bounding_rect` must return the union of those
//! fragments instead of a zero-sized rect.

use blitz_dom::{BaseDocument, DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; font-size: 16px; line-height: 20px; }
    #para { width: 100px; }
    #box { display: inline-block; width: 30px; height: 10px; }
</style></head>
<body>
<p id="para">aaaa aaaa <span id="wrapped">bbbb bbbb <b id="nested">cccc</b> <span id="box"></span> bbbb</span> aaaa</p>
</body></html>
"#;

fn make_doc() -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn node(doc: &BaseDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().unwrap()
}

#[test]
fn wrapped_span_has_one_rect_per_line() {
    let doc = make_doc();
    let span = node(&doc, "#wrapped");

    let rects = doc.node_client_rects(span);
    assert!(
        rects.len() >= 2,
        "span wrapped across a 100px-wide paragraph should fragment, got {rects:?}"
    );
    for rect in &rects {
        assert!(rect.width > 0.0, "fragment has zero width: {rect:?}");
        assert!(rect.height > 0.0, "fragment has zero height: {rect:?}");
    }
    // Fragments are on distinct line boxes, in document order
    for pair in rects.windows(2) {
        assert!(
            pair[1].y > pair[0].y,
            "fragments should be on successive lines: {rects:?}"
        );
    }
}

#[test]
fn bounding_rect_is_union_of_fragments() {
    let doc = make_doc();
    let span = node(&doc, "#wrapped");

    let rects = doc.node_client_rects(span);
    let bounding = doc.get_client_bounding_rect(span).unwrap();

    assert!(bounding.width > 0.0);
    assert!(bounding.height > 0.0);
    for rect in &rects {
        assert!(rect.x >= bounding.x - 0.01);
        assert!(rect.y >= bounding.y - 0.01);
        assert!(rect.x + rect.width <= bounding.x + bounding.width + 0.01);
        assert!(rect.y + rect.height <= bounding.y + bounding.height + 0.01);
    }
}

#[test]
fn fragments_include_nested_inline_and_atomic_children() {
    let doc = make_doc();
    let span = node(&doc, "#wrapped");
    let nested = node(&doc, "#nested");
    let atomic = node(&doc, "#box");

    let span_bounding = doc.get_client_bounding_rect(span).unwrap();

    // The nested <b> is itself a non-atomic inline: it gets fragment rects
    // that lie within the outer span's bounding rect
    let nested_rects = doc.node_client_rects(nested);
    assert!(!nested_rects.is_empty());
    for rect in &nested_rects {
        assert!(rect.x >= span_bounding.x - 0.01);
        assert!(rect.x + rect.width <= span_bounding.x + span_bounding.width + 0.01);
    }

    // The atomic inline-block has its own layout box (single rect) and is
    // included in the span's fragments
    let atomic_rects = doc.node_client_rects(atomic);
    assert_eq!(atomic_rects.len(), 1);
    let atomic_rect = &atomic_rects[0];
    assert_eq!(atomic_rect.width, 30.0);
    assert_eq!(atomic_rect.height, 10.0);
    assert!(atomic_rect.x >= span_bounding.x - 0.01);
    assert!(atomic_rect.x + atomic_rect.width <= span_bounding.x + span_bounding.width + 0.01);
    assert!(atomic_rect.y + atomic_rect.height <= span_bounding.y + span_bounding.height + 0.01);
}

#[test]
fn block_elements_return_single_rect() {
    let doc = make_doc();
    let para = node(&doc, "#para");

    let rects = doc.node_client_rects(para);
    assert_eq!(rects.len(), 1);
    let bounding = doc.get_client_bounding_rect(para).unwrap();
    assert_eq!(rects[0].x, bounding.x);
    assert_eq!(rects[0].y, bounding.y);
    assert_eq!(rects[0].width, bounding.width);
    assert_eq!(rects[0].height, bounding.height);
    assert_eq!(bounding.width, 100.0);
}
