//! `autofocus` is a boolean attribute: its presence alone should focus the
//! element when it is inserted. "false" is ignored because Dioxus writes
//! `autofocus: false` as that string.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use std::sync::Arc;

fn focussed_id(attr: &str) -> Option<String> {
    let html = format!(
        r#"<!DOCTYPE html><html><body><input type="text" id="first"><input type="text" id="target" {attr}></body></html>"#
    );
    let doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    let node_id = doc.get_focussed_node_id()?;
    doc.get_node(node_id)?
        .element_data()?
        .attr(markup5ever::local_name!("id"))
        .map(str::to_string)
}

#[test]
fn bare_autofocus_focusses_element() {
    assert_eq!(focussed_id("autofocus"), Some("target".into()));
}

#[test]
fn empty_autofocus_focusses_element() {
    assert_eq!(focussed_id(r#"autofocus="""#), Some("target".into()));
}

#[test]
fn true_autofocus_focusses_element() {
    assert_eq!(focussed_id(r#"autofocus="true""#), Some("target".into()));
}

#[test]
fn false_autofocus_is_ignored() {
    assert_eq!(focussed_id(r#"autofocus="false""#), None);
}

#[test]
fn no_autofocus_leaves_focus_unset() {
    assert_eq!(focussed_id(""), None);
}
