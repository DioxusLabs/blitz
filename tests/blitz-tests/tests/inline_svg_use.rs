//! A `<use>` in an inline `<svg>` may reference an element in another `<svg>` (an icon sprite).
//! Each inline `<svg>` reaches usvg as standalone markup, so the reference is expanded into a
//! clone that inherits from the `<use>` site, as a browser's shadow tree does.

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

fn paths(group: &usvg::Group, out: &mut Vec<usvg::Path>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => paths(group, out),
            usvg::Node::Path(path) => out.push(path.as_ref().clone()),
            _ => {}
        }
    }
}

fn icon_paths(doc: &HtmlDocument) -> Vec<usvg::Path> {
    let svg_id = doc.query_selector("#icon").unwrap().unwrap();
    let tree = doc
        .get_node(svg_id)
        .unwrap()
        .element_data()
        .unwrap()
        .svg_data()
        .expect("inline svg parsed");
    let mut out = Vec::new();
    paths(tree.root(), &mut out);
    out
}

fn fill(path: &usvg::Path) -> Option<(u8, u8, u8)> {
    match path.fill()?.paint() {
        usvg::Paint::Color(c) => Some((c.red, c.green, c.blue)),
        _ => None,
    }
}

const SPRITE: &str = r##"<svg width="0" height="0" style="position:absolute"><defs>
    <linearGradient id="g"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>
    <symbol id="dot" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="currentColor"/></symbol>
    <symbol id="bar" viewBox="0 0 10 10"><rect width="10" height="10" fill="url(#g)"/></symbol>
    <rect id="half" width="4" height="10" fill="red"/>
    <symbol id="loop" viewBox="0 0 10 10"><use href="#loop"/><rect width="10" height="10" fill="red"/></symbol>
</defs></svg>"##;

#[test]
fn use_renders_a_symbol_from_another_svg() {
    let doc = load(&format!(
        r##"<html><body style="color:rgb(255,0,0)">{SPRITE}<svg id="icon" width="20" height="20"><use href="#dot"/></svg></body></html>"##
    ));
    let paths = icon_paths(&doc);
    assert_eq!(
        paths.iter().map(fill).collect::<Vec<_>>(),
        vec![Some((255, 0, 0))]
    );
}

#[test]
fn use_content_inherits_from_the_use_site() {
    let doc = load(&format!(
        r##"<html><body style="color:rgb(255,0,0)">{SPRITE}
            <svg id="icon" width="20" height="20" style="color:rgb(0,0,255)"><use href="#dot"/></svg></body></html>"##
    ));
    assert_eq!(fill(&icon_paths(&doc)[0]), Some((0, 0, 255)));
}

#[test]
fn use_brings_paint_servers_from_another_svg() {
    let doc = load(&format!(
        r##"<html><body>{SPRITE}<svg id="icon" width="20" height="20"><use href="#bar"/></svg></body></html>"##
    ));
    let paths = icon_paths(&doc);
    assert!(matches!(
        paths[0].fill().map(|fill| fill.paint()),
        Some(usvg::Paint::LinearGradient(_))
    ));
}

#[test]
fn use_offsets_by_x_and_y_and_accepts_xlink_href() {
    let doc = load(&format!(
        r##"<html><body>{SPRITE}<svg id="icon" width="20" height="20"><use xlink:href="#half" x="6" y="2"/></svg></body></html>"##
    ));
    let paths = icon_paths(&doc);
    let bounds = paths[0].abs_bounding_box();
    assert_eq!((bounds.x(), bounds.y()), (6.0, 2.0));
}

#[test]
fn self_referencing_use_terminates() {
    let doc = load(&format!(
        r##"<html><body>{SPRITE}<svg id="icon" width="20" height="20"><use href="#loop"/></svg></body></html>"##
    ));
    assert!(!icon_paths(&doc).is_empty());
}

#[test]
fn use_added_after_parsing_is_expanded() {
    let mut doc = load(&format!(
        r##"<html><body>{SPRITE}<svg id="icon" width="20" height="20"></svg></body></html>"##
    ));
    let svg_id = doc.query_selector("#icon").unwrap().unwrap();
    doc.mutate()
        .set_inner_html(svg_id, r##"<use href="#half"/>"##);
    doc.resolve(0.0);
    assert_eq!(fill(&icon_paths(&doc)[0]), Some((255, 0, 0)));
}

#[test]
fn clone_keeps_its_target_id_for_selectors() {
    // `#half { … }` rules match the clone as they match the original, as in browsers. (Author
    // CSS paint reaching usvg is the separate fill/stroke serialization work.)
    let doc = load(&format!(
        r##"<html><body>{SPRITE}<svg id="icon" width="20" height="20"><use href="#half"/></svg></body></html>"##
    ));
    assert!(doc.query_selector("#icon use > #half").unwrap().is_some());
}

#[test]
fn a_later_use_resolves_to_the_original_not_a_clone() {
    // The clone keeps the target's id, but references still resolve to the original.
    let doc = load(&format!(
        r##"<html><body><svg width="20" height="20"><use href="#half"/></svg>{SPRITE}
            <svg id="icon" width="20" height="20"><use href="#half" x="6"/></svg></body></html>"##
    ));
    let paths = icon_paths(&doc);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].abs_bounding_box().x(), 6.0);
}
