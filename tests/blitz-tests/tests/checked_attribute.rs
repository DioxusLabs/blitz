//! The checked state of a checkbox or radio input follows its `checked`
//! attribute, both when the attribute is set and when it is removed.

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

fn checked() -> QualName {
    QualName::new(None, ns!(), local_name!("checked"))
}

fn is_checked(doc: &HtmlDocument, node: NodeId) -> (Option<bool>, bool) {
    let element = doc.get_node(node).unwrap().element_data().unwrap();
    let matches_checked = doc.query_selector("#t:checked").unwrap().is_some();
    (element.checkbox_input_checked(), matches_checked)
}

fn removing_checked_unchecks(input_type: &str) {
    let mut doc = make_doc(&format!(
        "<html><body><input id=t type={input_type} checked></body></html>"
    ));
    let node = node_id(&doc, "#t");
    assert_eq!(is_checked(&doc, node), (Some(true), true));

    doc.mutate().clear_attribute(node, checked());
    assert_eq!(is_checked(&doc, node), (Some(false), false));

    doc.mutate().set_attribute(node, checked(), "true");
    assert_eq!(is_checked(&doc, node), (Some(true), true));

    doc.mutate().clear_attribute(node, checked());
    assert_eq!(is_checked(&doc, node), (Some(false), false));
}

#[test]
fn removing_checked_unchecks_a_checkbox() {
    removing_checked_unchecks("checkbox");
}

#[test]
fn removing_checked_unchecks_a_radio() {
    removing_checked_unchecks("radio");
}
