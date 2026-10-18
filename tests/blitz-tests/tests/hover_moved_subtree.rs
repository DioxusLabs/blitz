//! Moving a hovered/active subtree to a new parent (without removing it first) must
//! not leave `:hover`/`:active` set on the ancestors it was moved away from.

use blitz_dom::{DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; }
    #item { height: 50px; }
    #a:hover, #b:hover { color: red; }
</style></head>
<body><div id="a"><div id="item"><span id="leaf">x</span></div></div><div id="b"></div></body></html>
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

fn node_id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().expect(selector)
}

fn is_hovered(doc: &HtmlDocument, id: NodeId) -> bool {
    doc.get_node(id).unwrap().is_hovered()
}

fn is_active(doc: &HtmlDocument, id: NodeId) -> bool {
    doc.get_node(id).unwrap().is_active()
}

#[test]
fn hover_moves_to_the_new_ancestors_of_a_moved_hovered_subtree() {
    let mut doc = make_doc();
    let [a, b, item] = ["#a", "#b", "#item"].map(|s| node_id(&doc, s));

    doc.set_hover_to(10.0, 10.0);
    assert!(is_hovered(&doc, item));
    assert!(is_hovered(&doc, a));

    // #a collapses, so #item stays under the pointer in its new parent.
    doc.mutate().append_children(b, &[item]);
    for resolved in [false, true] {
        assert_eq!(doc.get_hover_node_id(), Some(item));
        assert!(is_hovered(&doc, item), "resolved: {resolved}");
        assert!(is_hovered(&doc, b), "resolved: {resolved}");
        assert!(!is_hovered(&doc, a), "resolved: {resolved}");
        doc.resolve(0.0);
    }

    doc.set_hover_to(10.0, 250.0);
    for id in [a, b, item] {
        assert!(!is_hovered(&doc, id));
    }
}

#[test]
fn hover_is_cleared_from_a_subtree_moved_out_of_the_document() {
    let mut doc = make_doc();
    let [a, item, leaf] = ["#a", "#item", "#leaf"].map(|s| node_id(&doc, s));
    let detached = doc.mutate().create_element(
        markup5ever::QualName::new(
            None,
            markup5ever::ns!(html),
            markup5ever::local_name!("div"),
        ),
        vec![],
    );

    doc.set_hover_to(10.0, 10.0);
    assert!(is_hovered(&doc, item));

    doc.mutate().append_children(detached, &[item]);
    assert!(!is_hovered(&doc, leaf));
    assert!(!is_hovered(&doc, item));
    assert!(is_hovered(&doc, a));
    assert_eq!(doc.get_hover_node_id(), Some(a));

    doc.resolve(0.0);
    doc.set_hover_to(10.0, 250.0);
    assert!(!is_hovered(&doc, a));
}

#[test]
fn active_moves_to_the_new_ancestors_of_a_moved_active_subtree() {
    let mut doc = make_doc();
    let [a, b, item] = ["#a", "#b", "#item"].map(|s| node_id(&doc, s));

    doc.set_hover_to(10.0, 10.0);
    doc.active_node();
    assert!(is_active(&doc, item));
    assert!(is_active(&doc, a));

    doc.mutate().append_children(b, &[item]);
    assert!(is_active(&doc, item));
    assert!(is_active(&doc, b));
    assert!(!is_active(&doc, a));

    doc.resolve(0.0);
    doc.unactive_node();
    for id in [a, b, item] {
        assert!(!is_active(&doc, id));
    }
}
