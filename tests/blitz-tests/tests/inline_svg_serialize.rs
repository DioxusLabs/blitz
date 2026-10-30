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

fn first_stroke(group: &usvg::Group) -> Option<(u8, u8, u8)> {
    group.children().iter().find_map(|node| match node {
        usvg::Node::Group(group) => first_stroke(group),
        usvg::Node::Path(path) => match path.stroke()?.paint() {
            usvg::Paint::Color(c) => Some((c.red, c.green, c.blue)),
            _ => None,
        },
        _ => None,
    })
}

fn icon_stroke(doc: &HtmlDocument) -> Option<(u8, u8, u8)> {
    let svg_id = doc.query_selector("#icon").unwrap().unwrap();
    let tree = doc.get_node(svg_id).unwrap().element_data()?.svg_data()?;
    first_stroke(tree.root())
}

fn first_paint(group: &usvg::Group) -> Option<usvg::Paint> {
    group.children().iter().find_map(|node| match node {
        usvg::Node::Group(group) => first_paint(group),
        usvg::Node::Path(path) => Some(path.fill()?.paint().clone()),
        _ => None,
    })
}

fn icon_paint(doc: &HtmlDocument) -> Option<usvg::Paint> {
    let svg_id = doc.query_selector("#icon").unwrap().unwrap();
    let tree = doc.get_node(svg_id).unwrap().element_data()?.svg_data()?;
    first_paint(tree.root())
}

#[test]
fn inline_svg_css_fill_overrides_presentation_attribute() {
    let doc = load(
        r##"<html><head><style>#icon path { fill: rgb(0, 0, 204); }</style></head><body>
            <svg id="icon" width="24" height="24" viewBox="0 0 24 24" fill="none"><path fill="#080" d="M0 0h24v24H0z"/></svg>
        </body></html>"##,
    );
    assert_eq!(icon_fill(&doc), Some((0, 0, 204)));
}

#[test]
fn inline_svg_css_fill_on_root_is_inherited() {
    let doc = load(
        r##"<html><head><style>#icon { fill: #c00; }</style></head><body>
            <svg id="icon" width="24" height="24" viewBox="0 0 24 24" fill="none"><path d="M0 0h24v24H0z"/></svg>
        </body></html>"##,
    );
    assert_eq!(icon_fill(&doc), Some((204, 0, 0)));
}

#[test]
fn inline_svg_fill_attribute_is_inherited() {
    let doc = load(
        r##"<html><body>
            <svg id="icon" width="24" height="24" viewBox="0 0 24 24" fill="#080"><path d="M0 0h24v24H0z"/></svg>
        </body></html>"##,
    );
    assert_eq!(icon_fill(&doc), Some((0, 136, 0)));
}

#[test]
fn inline_svg_without_paint_defaults_to_black() {
    let doc = load(
        r##"<html><body>
            <svg id="icon" width="24" height="24" viewBox="0 0 24 24"><path d="M0 0h24v24H0z"/></svg>
        </body></html>"##,
    );
    assert_eq!(icon_fill(&doc), Some((0, 0, 0)));
}

#[test]
fn inline_svg_paint_server_fill_is_kept() {
    let doc = load(
        r##"<html><body>
            <svg id="icon" width="24" height="24" viewBox="0 0 24 24">
                <linearGradient id="g"><stop offset="0" stop-color="#00c"/><stop offset="1" stop-color="#c00"/></linearGradient>
                <path fill="url(#g)" d="M0 0h24v24H0z"/>
            </svg>
        </body></html>"##,
    );
    assert!(matches!(
        icon_paint(&doc),
        Some(usvg::Paint::LinearGradient(_))
    ));
}

#[test]
fn inline_svg_css_stroke_reaches_usvg() {
    let doc = load(
        r##"<html><head><style>#icon path { stroke: rgb(0, 0, 204); stroke-width: 2; }</style></head><body>
            <svg id="icon" width="24" height="24" viewBox="0 0 24 24"><path d="M0 0h24v24H0z"/></svg>
        </body></html>"##,
    );
    assert_eq!(icon_stroke(&doc), Some((0, 0, 204)));
}
