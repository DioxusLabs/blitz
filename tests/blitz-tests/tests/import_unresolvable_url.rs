//! An `@import` whose URL cannot be resolved (a relative URL with no usable base URL, or a
//! syntactically invalid absolute URL) is ignored rather than panicking, and the rest of the
//! stylesheet still applies (CSS Cascade 4 section 9.2).

use std::sync::Arc;

use blitz_dom::{CopyStyleGetters, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};

fn p_color(import: &str, base_url: Option<&str>) -> [f32; 3] {
    let html = format!(
        "<html><head><style>{import} p {{ color: rgb(1, 2, 3) }}</style></head>\
         <body><p>x</p></body></html>"
    );
    let config = DocumentConfig {
        base_url: base_url.map(Into::into),
        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
        ..Default::default()
    };
    let mut doc = HtmlDocument::from_html(&html, config).into_inner();
    doc.resolve(0.0);
    let p = doc.query_selector("p").unwrap().unwrap();
    let color = doc
        .get_node(p)
        .unwrap()
        .primary_styles()
        .unwrap()
        .clone_color();
    let c = color.components;
    [c.0, c.1, c.2]
}

fn assert_rule_applies(import: &str, base_url: Option<&str>) {
    let c = p_color(import, base_url);
    for (got, want) in c.iter().zip([1.0, 2.0, 3.0]) {
        assert!((got - want / 255.0).abs() < 1e-4, "{import}: {c:?}");
    }
}

#[test]
fn relative_string_without_base_url() {
    assert_rule_applies(r#"@import "foo.css";"#, None);
}

#[test]
fn relative_url_function_without_base_url() {
    assert_rule_applies("@import url(bar.css);", None);
}

#[test]
fn invalid_absolute_url_without_base_url() {
    assert_rule_applies(r#"@import "http://[";"#, None);
}

#[test]
fn invalid_absolute_url_with_base_url() {
    assert_rule_applies(r#"@import "http://[";"#, Some("http://example.com/"));
}
