//! `<input placeholder>` is laid out and shown while the field is empty.
//!
//! The attribute was previously ignored entirely, so an empty field rendered
//! as a blank box with no hint of what belonged in it.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn document(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

/// `(is the placeholder laid out, is it currently shown, the value)`
fn state(doc: &HtmlDocument, id: &str) -> (bool, bool, String) {
    let node = doc.get_element_by_id(id).unwrap();
    let input = doc.tree()[node]
        .element_data()
        .and_then(|el| el.text_input_data())
        .expect("an input has text input data");

    (
        input.placeholder.is_some(),
        input.shows_placeholder(),
        input.editor.text().to_string(),
    )
}

fn placeholder_color(doc: &HtmlDocument, id: &str) -> [f32; 4] {
    let node = doc.get_element_by_id(id).unwrap();
    doc.tree()[node]
        .element_data()
        .and_then(|el| el.text_input_data())
        .and_then(|input| input.placeholder.as_ref())
        .expect("the field has a placeholder")
        .color
        .components
}

#[test]
fn an_empty_input_shows_its_placeholder() {
    let doc = document(r#"<html><body><input id="a" placeholder="Search"></body></html>"#);
    let (laid_out, shown, value) = state(&doc, "a");

    assert!(laid_out, "the placeholder is laid out");
    assert!(shown, "an empty field shows the placeholder");
    assert_eq!(value, "", "the placeholder is never part of the value");
}

#[test]
fn a_value_hides_the_placeholder() {
    let doc =
        document(r#"<html><body><input id="a" value="Ada" placeholder="Search"></body></html>"#);
    let (laid_out, shown, value) = state(&doc, "a");

    assert!(laid_out);
    assert!(!shown, "a field with a value does not show the placeholder");
    assert_eq!(value, "Ada");
}

#[test]
fn an_input_without_a_placeholder_has_none() {
    let doc = document(r#"<html><body><input id="a"></body></html>"#);
    let (laid_out, shown, value) = state(&doc, "a");

    assert!(!laid_out);
    assert!(!shown);
    assert_eq!(
        value, "",
        "an empty input holds an empty string, not a space"
    );
}

#[test]
fn an_empty_placeholder_attribute_is_ignored() {
    let doc = document(r#"<html><body><input id="a" placeholder=""></body></html>"#);
    let (laid_out, shown, _) = state(&doc, "a");

    assert!(!laid_out);
    assert!(!shown);
}

#[test]
fn a_textarea_shows_its_placeholder() {
    let doc =
        document(r#"<html><body><textarea id="a" placeholder="Notes"></textarea></body></html>"#);
    let (laid_out, shown, _) = state(&doc, "a");

    assert!(laid_out);
    assert!(shown);
}

#[test]
fn placeholder_text_is_dimmer_than_the_value_by_default() {
    let doc = document(r##"<html><body><input id="a" placeholder="Search"></body></html>"##);

    let [.., alpha] = placeholder_color(&doc, "a");
    assert!(
        alpha < 1.0,
        "the UA sheet dims placeholder text, got alpha {alpha}"
    );
}

#[test]
fn a_stylesheet_can_restyle_the_placeholder() {
    let doc = document(
        r##"<html><head><style>
            #a::placeholder { color: rgb(255, 0, 0); }
        </style></head><body><input id="a" placeholder="Search"></body></html>"##,
    );

    let [red, green, blue, alpha] = placeholder_color(&doc, "a");
    assert_eq!(
        [red, green, blue, alpha],
        [1.0, 0.0, 0.0, 1.0],
        "::placeholder overrides the UA colour"
    );
}
