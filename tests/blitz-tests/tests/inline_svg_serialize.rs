//! Inline `<svg>` is rendered by serializing its subtree and parsing it with usvg, so the
//! serialization must be well-formed XML.

use std::sync::Arc;

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};

fn load(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn first_fill(group: &usvg::Group) -> Option<(u8, u8, u8)> {
    group.children().iter().find_map(|node| match node {
        usvg::Node::Group(group) => first_fill(group),
        usvg::Node::Path(path) => match path.fill()?.paint() {
            usvg::Paint::Color(c) => Some((c.red, c.green, c.blue)),
            _ => None,
        },
        _ => None,
    })
}

fn icon_fill(doc: &HtmlDocument) -> Option<(u8, u8, u8)> {
    let svg_id = doc.query_selector("#icon").unwrap().unwrap();
    let tree = doc.get_node(svg_id).unwrap().element_data()?.svg_data()?;
    first_fill(tree.root())
}

const ICON: &str = r#"<svg id="icon" width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M0 0h24v24H0z"/></svg>"#;

#[test]
fn inline_svg_with_special_characters_in_text_parses() {
    let doc = load(
        r#"<html><body><svg id="icon" width="24" height="24" viewBox="0 0 24 24" fill="black">
            <title>Tom &amp; Jerry &lt;3</title><path d="M0 0h24v24H0z"/>
        </svg></body></html>"#,
    );
    assert_eq!(icon_fill(&doc), Some((0, 0, 0)));
}

#[test]
fn inline_svg_with_xlink_href_parses() {
    let doc = load(
        r##"<html><body><svg id="icon" width="24" height="24" viewBox="0 0 24 24" fill="black">
            <defs><path id="p" d="M0 0h24v24H0z"/></defs><use xlink:href="#p"/>
        </svg></body></html>"##,
    );
    assert_eq!(icon_fill(&doc), Some((0, 0, 0)));
}

#[test]
fn inline_svg_resolves_current_color() {
    let doc = load(&format!(
        r#"<html><body><div style="color: rgb(255, 0, 0)">{ICON}</div></body></html>"#
    ));
    assert_eq!(icon_fill(&doc), Some((255, 0, 0)));
}

#[test]
fn outer_html_does_not_resolve_current_color() {
    let doc = load(&format!(
        r#"<html><body><div style="color: rgb(255, 0, 0)">{ICON}</div></body></html>"#
    ));
    let svg_id = doc.query_selector("#icon").unwrap().unwrap();
    let html = doc.get_node(svg_id).unwrap().outer_html();
    assert!(html.contains(r#"fill="currentColor""#), "{html}");
}

#[test]
fn page_css_reaches_inline_svg_shapes() {
    // Page CSS selecting a shape inside an inline <svg> applies, as in browsers.
    let doc = load(&format!(
        "<style>#icon path {{ fill: rgb(255, 0, 0) }}</style>{}",
        r#"<svg id="icon" width="24" height="24"><path d="M0 0h24v24H0z"/></svg>"#
    ));
    assert_eq!(icon_fill(&doc), Some((255, 0, 0)));
}

#[test]
fn page_css_beats_presentation_attributes() {
    // Author CSS wins over a `fill` presentation attribute.
    let doc = load(&format!(
        "<style>.shape {{ fill: rgb(0, 0, 255) }}</style>{}",
        r#"<svg id="icon" width="24" height="24"><path class="shape" fill="red" d="M0 0h24v24H0z"/></svg>"#
    ));
    assert_eq!(icon_fill(&doc), Some((0, 0, 255)));
}

#[test]
fn gradient_fills_from_the_markup_keep_working() {
    // A paint server stays as the markup has it: usvg resolves the fragment itself.
    let doc = load(
        r##"<svg id="icon" width="24" height="24"><defs><linearGradient id="g"><stop offset="0" stop-color="lime"/><stop offset="1" stop-color="lime"/></linearGradient></defs><path fill="url(#g)" d="M0 0h24v24H0z"/></svg>"##,
    );
    let svg_id = doc.query_selector("#icon").unwrap().unwrap();
    let source = doc.get_node(svg_id).unwrap().svg_source();
    assert!(!source.contains("fill:url"), "{source}");
    assert!(source.contains(r##"fill="url(#g)""##), "{source}");
}
