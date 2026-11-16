//! `HtmlDocument::from_html_without_sniffing` parses HTML as HTML whatever its DOCTYPE says, as
//! browsers do with a document served as `text/html`.
//!
//! Regression test: HTML mail often starts with an XHTML 1.0 Transitional DOCTYPE. Sniffed, it
//! was parsed as XML; with `xmlns="https://www.w3.org/1999/xhtml"` (https, not the XHTML
//! namespace) on its root, none of its elements were HTML: the `<head>`, the title and the style
//! sheets' text were laid out as content and the style sheets didn't apply.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn config() -> DocumentConfig {
    DocumentConfig {
        viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
        ..Default::default()
    }
}

const XHTML_DOCTYPE: &str = r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd">"#;

/// The width of `#p` and the height of the `<body>`.
fn p_width_and_body_height(head: &str, body: &str) -> (f32, f32) {
    let html = format!(
        r#"{XHTML_DOCTYPE}
<html xmlns="https://www.w3.org/1999/xhtml" lang="en">
<head>{head}<title>Mail</title><style type="text/css">#p {{ width: 123px; margin: 0; height: 20px }}</style></head>
<body style="margin:0">{body}</body>
</html>"#
    );
    let mut doc = HtmlDocument::from_html_without_sniffing(&html, config());
    doc.resolve(0.0);
    let layout = |sel: &str| {
        let id = doc.query_selector(sel).unwrap().expect(sel);
        doc.get_node(id).unwrap().final_layout()
    };
    (layout("#p").size.width, layout("body").size.height)
}

#[test]
fn an_xhtml_doctype_with_an_https_namespace_is_html() {
    let (width, height) = p_width_and_body_height("", r#"<p id="p"></p>"#);
    // The style sheet applies, and nothing from the head is laid out in the body.
    assert_eq!((width, height), (123.0, 20.0));
}

#[test]
fn html_that_isnt_well_formed_xml_is_parsed_as_html() {
    let (width, height) = p_width_and_body_height(
        r#"<meta charset="utf-8">"#,
        r#"<p id="p">a&nbsp;b<br>c</p>"#,
    );
    assert_eq!((width, height), (123.0, 20.0));
}
