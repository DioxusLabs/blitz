//! An inline `<svg>` is drawn from a snapshot of its source. A change to the
//! `<svg>`'s own transform (a spinning icon) keeps the snapshot; a change to
//! its content or to the colour it inherits rebuilds it.

use blitz_dom::node::ImageData;
use blitz_dom::{DocumentConfig, NodeId, QualName, local_name, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<html><head><style>
  @keyframes spin { to { transform: rotate(360deg); } }
  #icon { animation: spin 1s linear infinite; }
  .blue { color: blue; }
</style></head><body>
  <div id="wrap">
    <svg id="icon" width="24" height="24" viewBox="0 0 24 24">
      <circle id="dot" cx="12" cy="12" r="10" fill="currentColor"/>
    </svg>
  </div>
</body></html>"#;

fn make_doc() -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn node_id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().expect(selector)
}

/// The `<svg>`'s current snapshot. Holding the clone keeps the old tree alive,
/// so a rebuilt snapshot can't reuse its allocation.
fn snapshot(doc: &HtmlDocument) -> ImageData {
    let svg = node_id(doc, "#icon");
    doc.get_node(svg)
        .unwrap()
        .element_data()
        .unwrap()
        .image_data()
        .expect("the <svg> has a snapshot")
        .clone()
}

fn same_snapshot(a: &ImageData, b: &ImageData) -> bool {
    match (a, b) {
        (ImageData::Svg(a), ImageData::Svg(b)) => Arc::ptr_eq(&a.tree, &b.tree),
        _ => panic!("not SVG snapshots"),
    }
}

#[test]
fn a_transform_animation_keeps_the_snapshot() {
    let mut doc = make_doc();
    let before = snapshot(&doc);
    for t in [0.1, 0.2, 0.35, 0.5] {
        doc.resolve(t);
        assert!(
            same_snapshot(&before, &snapshot(&doc)),
            "rebuilt at t = {t}"
        );
    }
}

#[test]
fn a_changed_child_attribute_rebuilds_the_snapshot() {
    let mut doc = make_doc();
    let before = snapshot(&doc);
    let dot = node_id(&doc, "#dot");
    doc.mutate()
        .set_attribute(dot, QualName::new(None, ns!(), local_name!("r")), "6");
    doc.resolve(0.1);
    assert!(!same_snapshot(&before, &snapshot(&doc)));
}

#[test]
fn a_changed_svg_attribute_rebuilds_the_snapshot() {
    let mut doc = make_doc();
    let before = snapshot(&doc);
    let svg = node_id(&doc, "#icon");
    doc.mutate().set_attribute(
        svg,
        QualName::new(None, ns!(), local_name!("viewBox")),
        "0 0 12 12",
    );
    doc.resolve(0.1);
    assert!(!same_snapshot(&before, &snapshot(&doc)));
}

#[test]
fn an_inherited_colour_change_rebuilds_the_snapshot() {
    let mut doc = make_doc();
    let before = snapshot(&doc);
    let wrap = node_id(&doc, "#wrap");
    doc.mutate().set_attribute(
        wrap,
        QualName::new(None, ns!(), local_name!("class")),
        "blue",
    );
    doc.resolve(0.1);
    assert!(!same_snapshot(&before, &snapshot(&doc)));
}
