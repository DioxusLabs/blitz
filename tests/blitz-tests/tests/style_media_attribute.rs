//! A `<style>` sheet applies only while its `media` attribute matches.

use blitz_dom::{DocumentConfig, LocalName, QualName, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn make_doc(sheets: &str) -> HtmlDocument {
    let html = format!(
        r#"<!DOCTYPE html><html><head><style>body {{ margin: 0 }} #t {{ width: 100px }}</style>
{sheets}</head><body><div id="t"></div></body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn used_width(doc: &HtmlDocument) -> f32 {
    let id = doc.query_selector("#t").unwrap().unwrap();
    doc.get_node(id).unwrap().final_layout().size.width
}

fn media() -> QualName {
    QualName::new(None, ns!(), LocalName::from("media"))
}

#[test]
fn a_style_sheet_with_non_matching_media_does_not_apply() {
    let doc = make_doc(
        r#"<style media="print">#t { width: 10px }</style>
<style media="(max-width: 400px)">#t { width: 20px }</style>"#,
    );
    assert_eq!(used_width(&doc), 100.0);
}

#[test]
fn a_style_sheet_with_matching_media_applies() {
    let doc =
        make_doc(r#"<style media="screen and (min-width: 400px)">#t { width: 30px }</style>"#);
    assert_eq!(used_width(&doc), 30.0);
}

#[test]
fn changing_the_media_attribute_updates_the_sheet() {
    let mut doc = make_doc(r#"<style id="s" media="print">#t { width: 40px }</style>"#);
    let style = doc.query_selector("#s").unwrap().unwrap();

    doc.mutate().set_attribute(style, media(), "all");
    doc.resolve(0.0);
    assert_eq!(used_width(&doc), 40.0);

    doc.mutate().set_attribute(style, media(), "print");
    doc.resolve(0.0);
    assert_eq!(used_width(&doc), 100.0);

    doc.mutate().clear_attribute(style, media());
    doc.resolve(0.0);
    assert_eq!(used_width(&doc), 40.0);
}
