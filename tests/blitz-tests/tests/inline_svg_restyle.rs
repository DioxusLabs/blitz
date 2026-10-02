//! Inline `<svg>` is rendered from a snapshot of its subtree with `currentColor` resolved, so it
//! must be rebuilt when its styles change.

use std::sync::Arc;

use blitz_dom::{DocumentConfig, LocalName, QualName, ns};
use blitz_html::{HtmlDocument, HtmlProvider};

fn attr_name(local: &str) -> QualName {
    QualName {
        prefix: None,
        ns: ns!(),
        local: LocalName::from(local),
    }
}

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
fn inline_svg_current_color_follows_inherited_color_change() {
    let mut doc = load(&format!(
        r#"<html><body><div id="parent" style="color: rgb(255, 0, 0)">{ICON}</div></body></html>"#
    ));
    assert_eq!(icon_fill(&doc), Some((255, 0, 0)));

    let parent = doc.query_selector("#parent").unwrap().unwrap();
    doc.mutate()
        .set_attribute(parent, attr_name("style"), "color: rgb(0, 0, 255)");
    doc.resolve(0.0);
    assert_eq!(icon_fill(&doc), Some((0, 0, 255)));
}

#[test]
fn inline_svg_current_color_follows_class_change() {
    let mut doc = load(&format!(
        r#"<html><head><style>.on {{ color: rgb(0, 128, 0) }}</style></head>
        <body><div id="parent">{ICON}</div></body></html>"#
    ));
    assert_eq!(icon_fill(&doc), Some((0, 0, 0)));

    let parent = doc.query_selector("#parent").unwrap().unwrap();
    doc.mutate().set_attribute(parent, attr_name("class"), "on");
    doc.resolve(0.0);
    assert_eq!(icon_fill(&doc), Some((0, 128, 0)));
}
