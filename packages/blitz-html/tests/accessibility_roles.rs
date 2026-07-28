//! HTML elements map to their AccessKit roles.
//!
//! Only a handful of elements were mapped, so links, lists, tables, labels and
//! landmarks all arrived as [`Role::Unknown`] and assistive technology had
//! nothing to navigate by.
//!
//! <https://www.w3.org/TR/html-aam-1.0/>

#![cfg(feature = "accessibility")]

use accesskit::{NodeId, Role};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::collections::HashMap;
use std::sync::Arc;

/// The `html_tag` of every element that still maps to [`Role::Unknown`].
fn unknown_tags(html: &str) -> Vec<String> {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    let mut tags: Vec<String> = doc
        .build_accessibility_tree()
        .nodes
        .into_iter()
        .filter(|(_, node)| node.role() == Role::Unknown)
        .filter_map(|(_, node)| node.html_tag().map(|tag| tag.to_string()))
        .collect();
    tags.sort();
    tags
}

/// Resolve the role of the element matching `id` in the document.
#[track_caller]
fn assert_role(html: &str, element_id: &str, expected: Role) {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    let node_id = doc
        .get_element_by_id(element_id)
        .unwrap_or_else(|| panic!("no element with id {element_id}"));

    let roles: HashMap<NodeId, Role> = doc
        .build_accessibility_tree()
        .nodes
        .into_iter()
        .map(|(id, node)| (id, node.role()))
        .collect();

    let actual = roles
        .get(&NodeId(node_id.as_u64()))
        .copied()
        .unwrap_or(Role::Unknown);

    assert_eq!(
        actual, expected,
        "#{element_id}: expected {expected:?} but got {actual:?}"
    );
}

#[test]
fn landmarks_and_structure() {
    let html = r#"<html><body>
        <nav id="nav"></nav>
        <main id="main"></main>
        <aside id="aside"></aside>
        <footer id="footer"></footer>
        <article id="article"></article>
        <blockquote id="quote"></blockquote>
        <figure id="figure"></figure>
    </body></html>"#;

    assert_role(html, "nav", Role::Navigation);
    assert_role(html, "main", Role::Main);
    assert_role(html, "aside", Role::Complementary);
    assert_role(html, "footer", Role::Footer);
    assert_role(html, "article", Role::Article);
    assert_role(html, "quote", Role::Blockquote);
    assert_role(html, "figure", Role::Figure);
}

#[test]
fn lists_and_tables() {
    let html = r#"<html><body>
        <ul id="ul"><li id="li">item</li></ul>
        <ol id="ol"></ol>
        <table id="table">
            <thead id="thead"><tr id="tr"><th id="col">Key</th></tr></thead>
            <tbody><tr><td id="cell">Value</td><th id="row" scope="row">R</th></tr></tbody>
        </table>
    </body></html>"#;

    assert_role(html, "ul", Role::List);
    assert_role(html, "ol", Role::List);
    assert_role(html, "li", Role::ListItem);
    assert_role(html, "table", Role::Table);
    assert_role(html, "thead", Role::RowGroup);
    assert_role(html, "tr", Role::Row);
    assert_role(html, "col", Role::ColumnHeader);
    assert_role(html, "cell", Role::Cell);
    assert_role(html, "row", Role::RowHeader);
}

#[test]
fn an_anchor_is_a_link_only_with_an_href() {
    let html = r##"<html><body>
        <a id="link" href="#x">link</a>
        <a id="anchor">not a link</a>
    </body></html>"##;

    assert_role(html, "link", Role::Link);
    assert_role(html, "anchor", Role::GenericContainer);
}

#[test]
fn form_controls() {
    let html = r#"<html><body>
        <label id="label">Name</label>
        <select id="select"></select>
        <select id="multi" multiple></select>
        <textarea id="textarea"></textarea>
        <progress id="progress"></progress>
        <meter id="meter"></meter>
        <input id="radio" type="radio">
        <input id="range" type="range">
        <input id="email" type="email">
        <input id="password" type="password">
        <input id="submit" type="submit">
    </body></html>"#;

    assert_role(html, "label", Role::Label);
    assert_role(html, "select", Role::ComboBox);
    assert_role(html, "multi", Role::ListBox);
    assert_role(html, "textarea", Role::MultilineTextInput);
    assert_role(html, "progress", Role::ProgressIndicator);
    assert_role(html, "meter", Role::Meter);
    assert_role(html, "radio", Role::RadioButton);
    assert_role(html, "range", Role::Slider);
    assert_role(html, "email", Role::EmailInput);
    assert_role(html, "password", Role::PasswordInput);
    assert_role(html, "submit", Role::Button);
}

#[test]
fn previously_mapped_roles_are_unchanged() {
    let html = r#"<html><body>
        <button id="button"></button>
        <div id="div"></div>
        <header id="header"></header>
        <h2 id="heading"></h2>
        <p id="para"></p>
        <section id="section"></section>
        <input id="text" type="text">
        <input id="number" type="number">
        <input id="checkbox" type="checkbox">
    </body></html>"#;

    assert_role(html, "button", Role::Button);
    assert_role(html, "div", Role::GenericContainer);
    assert_role(html, "header", Role::Header);
    assert_role(html, "heading", Role::Heading);
    assert_role(html, "para", Role::Paragraph);
    assert_role(html, "section", Role::Section);
    assert_role(html, "text", Role::TextInput);
    assert_role(html, "number", Role::NumberInput);
    assert_role(html, "checkbox", Role::CheckBox);
}

#[test]
fn a_semantic_page_has_no_unknown_elements() {
    let html = r##"<html><body>
        <header><h1>Title</h1><nav><a href="#a">One</a></nav></header>
        <main>
            <p>Body</p>
            <ul><li>Item</li></ul>
            <table><tr><th>K</th><td>V</td></tr></table>
            <label>Name <input type="text"></label>
        </main>
        <footer>End</footer>
    </body></html>"##;

    // <html>, <head> and <body> have no roles of their own. Everything else in
    // this document should map to something an assistive technology can use.
    assert_eq!(unknown_tags(html), vec!["body", "head", "html"]);
}
