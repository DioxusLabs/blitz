//! Whether an element can be focused is cached on it, and follows the
//! attributes it is computed from.
//!
//! A widget that hands the focus around its own children sets their tabindex
//! after creating them - a roving tabindex does - so a cache that is only
//! filled at creation leaves every one of them unfocusable.

use blitz_dom::{DocumentConfig, NodeId, QualName, local_name, ns};
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

fn tabindex() -> QualName {
    QualName::new(None, ns!(), local_name!("tabindex"))
}

#[test]
fn a_tabindex_set_after_creation_takes_effect() {
    let mut doc = make_doc("<html><body><div id=t>cell</div></body></html>");
    let node = node_id(&doc, "#t");
    assert!(!doc.get_node(node).unwrap().is_focussable());

    doc.mutate().set_attribute(node, tabindex(), "0");

    assert!(
        doc.get_node(node).unwrap().is_focussable(),
        "the element did not become focusable"
    );
}

#[test]
fn removing_it_takes_the_focusability_away_again() {
    let mut doc = make_doc("<html><body><div id=t tabindex=0>cell</div></body></html>");
    let node = node_id(&doc, "#t");
    assert!(doc.get_node(node).unwrap().is_focussable());

    doc.mutate().clear_attribute(node, tabindex());

    assert!(!doc.get_node(node).unwrap().is_focussable());
}

/// Disabling an element takes it out of both.
///
/// Note the value: blitz reads `disabled` as a parsed boolean rather than
/// treating the bare attribute as disabling, so `""` would not count.
#[test]
fn disabling_it_takes_it_out_of_both() {
    let mut doc = make_doc("<html><body><button id=t>press</button></body></html>");
    let node = node_id(&doc, "#t");
    assert!(doc.get_node(node).unwrap().is_focussable());

    doc.mutate().set_attribute(
        node,
        QualName::new(None, ns!(), local_name!("disabled")),
        "true",
    );

    assert!(!doc.get_node(node).unwrap().is_focussable());
}
