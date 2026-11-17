//! HTML elements map to their AccessKit roles.
//!
//! Only a handful of elements were mapped, so links, lists, tables, labels and
//! landmarks all arrived as [`Role::Unknown`] and assistive technology had
//! nothing to navigate by.
//!
//! <https://www.w3.org/TR/html-aam-1.0/>

use accesskit::{NodeId, Role};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use rstest::rstest;
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

    // <html> and <body> have no roles of their own. Everything else in
    // this document should map to something an assistive technology can use.
    assert_eq!(unknown_tags(html), vec!["body", "html"]);
}

/// The `role` attribute is a list of tokens, of which the first recognized one applies.
///
/// <https://www.w3.org/TR/wai-aria-1.2/#host_general_role>
#[rstest]
#[case::second_token("arbitrary-unknown-role button", Role::Button)]
#[case::third_token("arbitrary-unknown-role other-unknown-role button", Role::Button)]
#[case::first_of_two_recognized_tokens("button link", Role::Button)]
#[case::second_of_two_recognized_tokens("link button", Role::Link)]
#[case::separated_by_tab("arbitrary-unknown-role&#9;button", Role::Button)]
#[case::separated_by_line_feed("arbitrary-unknown-role&#10;button", Role::Button)]
#[case::separated_by_several_spaces("arbitrary-unknown-role   button", Role::Button)]
#[case::surrounding_whitespace("  button  ", Role::Button)]
fn role_attribute_resolves_to_first_recognized_role(#[case] role: &str, #[case] expected: Role) {
    let html = format!(r#"<html><body><div id="element" role="{role}"></div></body></html>"#);

    assert_role(&html, "element", expected);
}

/// Without any recognized token, the native role of the element applies.
#[rstest]
#[case::unknown_role("arbitrary-unknown-role")]
#[case::several_unknown_roles("arbitrary-unknown-role other-unknown-role")]
#[case::empty("")]
#[case::whitespace_only("   ")]
fn role_attribute_without_recognized_role_falls_back_to_native_role(#[case] role: &str) {
    let html = format!(
        r#"<html><body><div id="div" role="{role}"></div><button id="button" role="{role}"></button></body></html>"#
    );

    assert_role(&html, "div", Role::GenericContainer);
    assert_role(&html, "button", Role::Button);
}

/// Neither ARIA nor HTML requires this, but browsers compare role tokens ASCII
/// case-insensitively.
///
/// <https://www.w3.org/TR/html-aria/#case-requirements-for-aria-role-state-and-property-attributes>
#[rstest]
#[case::upper_case("BUTTON", Role::Button)]
#[case::mixed_case("Button", Role::Button)]
#[case::textbox("TextBox", Role::TextInput)]
#[case::searchbox("SEARCHBOX", Role::SearchInput)]
#[case::case_in_list("Arbitrary-Unknown-Role LINK", Role::Link)]
fn role_attribute_ignores_case(#[case] role: &str, #[case] expected: Role) {
    let html = format!(r#"<html><body><div id="element" role="{role}"></div></body></html>"#);

    assert_role(&html, "element", expected);
}

/// The keywords of the `type` attribute are ASCII case-insensitive.
///
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#keywords-and-enumerated-attributes>
#[rstest]
#[case::checkbox("CHECKBOX", Role::CheckBox)]
#[case::radio("Radio", Role::RadioButton)]
#[case::range("RANGE", Role::Slider)]
#[case::color("Color", Role::ColorWell)]
#[case::date("DATE", Role::DateInput)]
#[case::datetime_local("DateTime-Local", Role::DateTimeInput)]
#[case::time("TIME", Role::TimeInput)]
#[case::email("EMAIL", Role::EmailInput)]
#[case::number("Number", Role::NumberInput)]
#[case::password("PASSWORD", Role::PasswordInput)]
#[case::search("Search", Role::SearchInput)]
#[case::tel("TEL", Role::PhoneNumberInput)]
#[case::text("TEXT", Role::TextInput)]
#[case::button("BUTTON", Role::Button)]
#[case::submit("Submit", Role::Button)]
#[case::reset("RESET", Role::Button)]
fn input_type_ignores_case(#[case] input_type: &str, #[case] expected: Role) {
    let html = format!(r#"<html><body><input id="input" type="{input_type}"></body></html>"#);

    assert_role(&html, "input", expected);
}

/// Chromium gives `file` and `image` inputs the role `button`, and `month` and `week` inputs a
/// date and time role, so they must not fall back to a text box.
#[rstest]
#[case::file("file", Role::Button)]
#[case::image("image", Role::Button)]
#[case::month("month", Role::MonthInput)]
#[case::week("week", Role::WeekInput)]
fn input_type_without_text_box_role(#[case] input_type: &str, #[case] expected: Role) {
    let html = format!(r#"<html><body><input id="input" type="{input_type}"></body></html>"#);

    assert_role(&html, "input", expected);
}

#[test]
fn an_image_with_an_empty_alt_is_decorative() {
    let html = r#"<html><body>
        <img id="decorative" alt="" src="data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%2F%3E">
        <img id="described" alt="A map" src="data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%2F%3E">
        <img id="unlabelled" src="data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%2F%3E">
    </body></html>"#;
    assert_role(html, "decorative", Role::GenericContainer);
    assert_role(html, "described", Role::Image);
    assert_role(html, "unlabelled", Role::Image);
}
