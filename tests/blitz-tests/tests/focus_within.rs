//! `:focus-within` matches the focused element and everything that contains
//! it in the DOM tree, and follows the focus as it moves - between elements,
//! with a subtree that is relocated, and away entirely when the focused node
//! leaves the document.
//!
//! The state travels the DOM parent chain rather than the layout chain: a
//! `display: contents` element has no box (and so no layout parent link), and
//! layout parent links do not exist at all before the first layout pass, which
//! is when `autofocus` fires.

use blitz_dom::{DocumentConfig, NodeId, QualName, local_name, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn make_doc(html: &str) -> HtmlDocument {
    HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    )
}

fn node_id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().expect(selector)
}

fn matches(doc: &HtmlDocument, selector: &str) -> bool {
    doc.query_selector(selector).unwrap().is_some()
}

#[test]
fn focus_within_matches_the_ancestors_and_clears_on_blur() {
    let mut doc = make_doc(
        "<html><body><div id=wrapper><input id=input></div>\
         <div id=other><input id=other_input></div></body></html>",
    );
    doc.resolve(0.0);

    let focus_target = node_id(&doc, "#input");
    doc.set_focus_to(focus_target);
    doc.resolve(0.0);
    for selector in [
        "#input:focus-within",
        "#wrapper:focus-within",
        "body:focus-within",
    ] {
        assert!(matches(&doc, selector), "{selector} does not match");
    }
    assert!(!matches(&doc, "#other:focus-within"));

    // Refocus moves the state over to the other branch
    let focus_target = node_id(&doc, "#other_input");
    doc.set_focus_to(focus_target);
    doc.resolve(0.0);
    assert!(!matches(&doc, "#wrapper:focus-within"));
    assert!(matches(&doc, "#other:focus-within"));

    doc.clear_focus();
    doc.resolve(0.0);
    assert!(!matches(&doc, "#other:focus-within"));
    assert!(!matches(&doc, "body:focus-within"));
}

#[test]
fn focus_within_matches_a_display_contents_ancestor() {
    let mut doc = make_doc(
        "<html><body><div id=wrapper style=\"display: contents\">\
         <input id=input></div></body></html>",
    );
    doc.resolve(0.0);

    let focus_target = node_id(&doc, "#input");
    doc.set_focus_to(focus_target);
    doc.resolve(0.0);
    assert!(
        matches(&doc, "#wrapper:focus-within"),
        "a display: contents element has no box, but it does contain the focus"
    );
}

#[test]
fn focus_within_matches_when_focus_is_set_before_the_first_layout_pass() {
    let mut doc = make_doc("<html><body><div id=wrapper><input id=input></div></body></html>");

    // No resolve() yet, as when autofocus fires or an embedder focuses a
    // field at startup.
    let focus_target = node_id(&doc, "#input");
    doc.set_focus_to(focus_target);
    assert!(
        matches(&doc, "#wrapper:focus-within"),
        "the state must not depend on layout parent links existing"
    );
}

#[test]
fn removing_the_subtree_containing_the_focus_clears_the_ancestors() {
    let mut doc =
        make_doc("<html><body><div id=wrapper><div id=inner><input></div></div></body></html>");
    doc.resolve(0.0);

    let input = node_id(&doc, "#wrapper input");
    doc.set_focus_to(input);
    doc.resolve(0.0);
    assert!(matches(&doc, "#wrapper:focus-within"));

    let inner = node_id(&doc, "#inner");
    doc.mutate().remove_node(inner);
    doc.resolve(0.0);
    // get_focussed_node_id falls back to the root element when nothing is focused
    assert_ne!(doc.get_focussed_node_id(), Some(input));
    assert!(!matches(&doc, "#wrapper:focus-within"));
    assert!(!matches(&doc, "body:focus-within"));
}

#[test]
fn moving_the_focused_subtree_takes_the_state_along() {
    let mut doc = make_doc(
        "<html><body><div id=a><div id=item><input></div></div>\
         <div id=b></div></body></html>",
    );
    doc.resolve(0.0);

    let focus_target = node_id(&doc, "#item input");
    doc.set_focus_to(focus_target);
    doc.resolve(0.0);
    assert!(matches(&doc, "#a:focus-within"));
    assert!(!matches(&doc, "#b:focus-within"));

    let item = node_id(&doc, "#item");
    let b = node_id(&doc, "#b");
    doc.mutate().append_children(b, &[item]);
    doc.resolve(0.0);
    assert!(matches(&doc, "#item input:focus"), "the focus itself stays");
    assert!(
        !matches(&doc, "#a:focus-within"),
        "the state stayed behind on the old ancestors"
    );
    assert!(
        matches(&doc, "#b:focus-within"),
        "the state did not reach the new ancestors"
    );
}

#[test]
fn moving_the_focused_subtree_out_of_the_document_clears_the_state() {
    let mut doc =
        make_doc("<html><body><div id=wrapper><div id=item><input></div></div></body></html>");
    doc.resolve(0.0);

    let focus_target = node_id(&doc, "#item input");
    doc.set_focus_to(focus_target);
    doc.resolve(0.0);
    assert!(matches(&doc, "#wrapper:focus-within"));

    // Reparent the subtree under a node that is not in the document
    let item = node_id(&doc, "#item");
    let detached = doc.mutate().create_element(
        QualName::new(None, ns!(html), local_name!("div")),
        Vec::new(),
    );
    doc.mutate().append_children(detached, &[item]);
    doc.resolve(0.0);
    // get_focussed_node_id falls back to the root element when nothing is focused
    assert_ne!(doc.get_focussed_node_id(), Some(focus_target));
    assert!(!matches(&doc, "#wrapper:focus-within"));
    assert!(!matches(&doc, "body:focus-within"));
}
