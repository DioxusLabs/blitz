//! Elements with no CSS layout box (detached, `display: none`, inside a
//! `display: none` ancestor, or `display: contents`) must report empty/zero/null
//! CSSOM geometry and ignore programmatic scrolling, even if they previously had
//! layout or scroll offsets.

use blitz_dom::{
    BaseDocument, DocumentConfig, NodeId, ScrollBehavior, ScrollLogicalPosition, qual_name,
};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; }
    #container {
        position: relative;
        margin: 20px;
        width: 200px;
        height: 200px;
        border: 5px solid black;
        overflow: scroll;
    }
    #child {
        width: 400px;
        height: 400px;
        border: 4px solid blue;
    }
    #contents {
        display: contents;
        border: 6px solid red;
    }
    #contents-child {
        width: 80px;
        height: 40px;
        border: 2px solid green;
    }
</style></head>
<body>
    <div id="container">
        <div id="child">hello</div>
    </div>
    <div id="contents">
        <div id="contents-child">visible</div>
    </div>
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

fn assert_boxless_geometry(doc: &BaseDocument, id: NodeId) {
    let n = doc.get_node(id).unwrap();
    assert!(!n.has_boxes());
    assert!(doc.get_client_bounding_rect(id).is_none());
    assert!(doc.node_client_rects(id).is_empty());
    assert!(doc.offset_rect(id).is_none());
    assert_eq!(n.offset_parent().map(|p| p.id), None);
    let pos = n.offset_top_left();
    assert_eq!((pos.x, pos.y), (0.0, 0.0));
    assert_eq!(n.client_top(), 0.0);
    assert_eq!(n.client_left(), 0.0);
    assert_eq!(n.client_width(), 0.0);
    assert_eq!(n.client_height(), 0.0);
    assert_eq!(n.scroll_width(), 0.0);
    assert_eq!(n.scroll_height(), 0.0);
}

#[test]
fn display_none_and_descendant_return_empty_geometry() {
    let mut doc = make_doc();
    let container = node(&doc, "#container");
    let child = node(&doc, "#child");

    doc.scroll_to(container, 10.0, 25.0, ScrollBehavior::Instant);
    assert!(doc.get_client_bounding_rect(container).is_some());
    assert!(doc.get_client_bounding_rect(child).is_some());

    doc.mutate()
        .set_attribute(container, qual_name!("style"), "display: none");
    doc.resolve(0.0);

    assert_boxless_geometry(&doc, container);
    assert_boxless_geometry(&doc, child);

    // Programmatic scrolling on boxless nodes must be a no-op
    let before = *doc.get_node(container).unwrap().scroll_offset();
    doc.scroll_to(container, 50.0, 60.0, ScrollBehavior::Instant);
    doc.scroll_by(container, 5.0, 5.0, ScrollBehavior::Instant);
    doc.scroll_into_view(
        child,
        ScrollBehavior::Instant,
        ScrollLogicalPosition::Start,
        ScrollLogicalPosition::Start,
    );
    assert_eq!(*doc.get_node(container).unwrap().scroll_offset(), before);
    assert_eq!(doc.viewport_scroll().x, 0.0);
    assert_eq!(doc.viewport_scroll().y, 0.0);
}

#[test]
fn detached_subtree_returns_empty_geometry() {
    let mut doc = make_doc();
    let container = node(&doc, "#container");
    let child = node(&doc, "#child");
    assert!(doc.get_client_bounding_rect(child).is_some());

    doc.mutate().remove_node(container);
    doc.resolve(0.0);

    assert_boxless_geometry(&doc, container);
    assert_boxless_geometry(&doc, child);
}

#[test]
fn display_contents_is_boxless_but_its_child_has_boxes() {
    let doc = make_doc();
    let contents = node(&doc, "#contents");
    let contents_child = node(&doc, "#contents-child");

    assert_boxless_geometry(&doc, contents);

    let child_rect = doc
        .get_client_bounding_rect(contents_child)
        .expect("child of display: contents should still have a box");
    assert_eq!(child_rect.width, 84.0);
    assert_eq!(child_rect.height, 44.0);
}
