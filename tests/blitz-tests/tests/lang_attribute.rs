//! The `lang` and `xml:lang` attributes set the element's language through
//! Stylo's internal, inherited `-x-lang` property, which text layout reads.

use blitz_dom::{QualName, local_name, ns};
use blitz_test_harness::Harness;

fn lang_of(harness: &Harness, selector: &str) -> String {
    let id = harness.node(selector);
    let doc = harness.base();
    let styles = doc.get_node(id).unwrap().primary_styles().unwrap();
    styles.get_font()._x_lang.0.to_string()
}

fn page(html_attrs: &str, body: &str) -> Harness {
    Harness::from_html(&format!("<html {html_attrs}><body>{body}</body></html>"))
}

#[test]
fn without_the_attribute_the_language_is_unknown() {
    let harness = page("", "<p id=p>text</p>");
    assert_eq!(lang_of(&harness, "#p"), "");
}

#[test]
fn the_language_is_inherited_from_the_root() {
    let harness = page("lang=ja", "<div><p id=p>text</p></div>");
    assert_eq!(lang_of(&harness, "#p"), "ja");
}

#[test]
fn an_inner_attribute_overrides_the_inherited_language() {
    let harness = page(
        "lang=en",
        "<div lang=zh-Hant><p id=p>text</p></div><p id=outer>text</p>",
    );
    assert_eq!(lang_of(&harness, "#p"), "zh-Hant");
    assert_eq!(lang_of(&harness, "#outer"), "en");
}

/// `lang=""` means the language is unknown, rather than inheriting.
#[test]
fn an_empty_attribute_resets_to_unknown() {
    let harness = page("lang=ja", "<div lang=''><p id=p>text</p></div>");
    assert_eq!(lang_of(&harness, "#p"), "");
}

/// `xml:lang` in the XML namespace takes precedence over `lang`.
#[test]
fn xml_lang_takes_precedence_over_lang() {
    let mut harness = page("", "<p id=p lang=en>text</p>");
    let p = harness.node("#p");
    harness.base_mut().mutate().set_attribute(
        p,
        QualName::new(None, ns!(xml), local_name!("lang")),
        "ja",
    );
    harness.pump();
    assert_eq!(lang_of(&harness, "#p"), "ja");
}

/// Precedence does not depend on attribute order.
#[test]
fn xml_lang_takes_precedence_over_a_later_lang() {
    let mut harness = page("", "<p id=p>text</p>");
    let p = harness.node("#p");
    harness.base_mut().mutate().set_attribute(
        p,
        QualName::new(None, ns!(xml), local_name!("lang")),
        "ja",
    );
    harness.base_mut().mutate().set_attribute(
        p,
        QualName::new(None, ns!(), local_name!("lang")),
        "en",
    );
    harness.pump();
    assert_eq!(lang_of(&harness, "#p"), "ja");
}

#[test]
fn changing_the_attribute_at_runtime_restyles_descendants() {
    let mut harness = page("lang=en", "<div id=d><p id=p>text</p></div>");
    assert_eq!(lang_of(&harness, "#p"), "en");

    let d = harness.node("#d");
    harness.base_mut().mutate().set_attribute(
        d,
        QualName::new(None, ns!(), local_name!("lang")),
        "ja",
    );
    harness.pump();
    assert_eq!(lang_of(&harness, "#p"), "ja");

    harness
        .base_mut()
        .mutate()
        .clear_attribute(d, QualName::new(None, ns!(), local_name!("lang")));
    harness.pump();
    assert_eq!(lang_of(&harness, "#p"), "en");
}
