//! `:hover` and `:active` propagate along DOM ancestors, not the box tree, so
//! elements that the box tree skips (e.g. `display:contents` wrappers, inline
//! ancestors of an atomic inline box) still match.

use blitz_dom::{DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn make_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
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
fn hover_propagates_to_display_contents_ancestor() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html><html><body style="margin:0">
        <div id="wrapper" style="display:contents"><div id="inner" style="height:50px"></div></div>
        </body></html>"#,
    );
    let wrapper = node_id(&doc, "#wrapper");
    let inner = node_id(&doc, "#inner");

    doc.set_hover_to(10.0, 10.0);
    assert_eq!(doc.get_hover_node_id(), Some(inner));
    assert!(is_hovered(&doc, inner));
    assert!(is_hovered(&doc, wrapper));

    doc.set_hover_to(10.0, 200.0);
    assert!(!is_hovered(&doc, inner));
    assert!(!is_hovered(&doc, wrapper));
}

#[test]
fn hover_and_active_propagate_to_inline_ancestors_of_inline_box() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html><html><body style="margin:0">
        <p id="p" style="margin:0"><a id="a" href="/link"><span id="icon" style="display:inline-block;width:30px;height:30px"></span></a></p>
        </body></html>"#,
    );
    let ids = [
        node_id(&doc, "#p"),
        node_id(&doc, "#a"),
        node_id(&doc, "#icon"),
    ];

    doc.set_hover_to(10.0, 10.0);
    assert_eq!(doc.get_hover_node_id(), Some(ids[2]));
    for id in ids {
        assert!(is_hovered(&doc, id));
    }

    doc.active_node();
    for id in ids {
        assert!(is_active(&doc, id));
    }
    doc.unactive_node();
    for id in ids {
        assert!(!is_active(&doc, id));
    }

    doc.set_hover_to(10.0, 200.0);
    for id in ids {
        assert!(!is_hovered(&doc, id));
    }
}
