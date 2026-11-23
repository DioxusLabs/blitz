//! Blitz's use of the `parley_css_text` offset map: the parts that depend on Blitz's box tree
//! and Stylo. The collapsing and mapping themselves are tested in `parley_css_text`.

use blitz_dom::node::TextLayout;
use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            font_ctx: Some(FontContext::new()),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn inline_layout(doc: &HtmlDocument) -> &TextLayout {
    let root = doc.query_selector("#t").unwrap().unwrap();
    doc.get_node(root)
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_deref()
        .unwrap()
}

/// The ids of the text nodes in `#t`, in tree order.
fn text_nodes(doc: &HtmlDocument) -> Vec<NodeId> {
    fn visit(doc: &HtmlDocument, id: NodeId, out: &mut Vec<NodeId>) {
        let node = doc.get_node(id).unwrap();
        if node.text_data().is_some() {
            out.push(id);
        }
        for &child in &node.children {
            visit(doc, child, out);
        }
    }
    let mut out = Vec::new();
    visit(doc, doc.query_selector("#t").unwrap().unwrap(), &mut out);
    out
}

/// The layout offset of every byte offset of `node`.
fn layout_offsets(layout: &TextLayout, node: NodeId, len: usize) -> Vec<usize> {
    (0..=len)
        .map(|source| layout.offset_map.layout_offset(node, source).unwrap())
        .collect()
}

#[test]
fn list_marker_text_comes_first() {
    let doc = layout_doc("<ul><li id=t style='list-style-position:inside'>x</li></ul>");
    let layout = inline_layout(&doc);
    let [node] = text_nodes(&doc)[..] else {
        panic!()
    };
    let marker_len = layout.text.len() - 1;
    assert!(marker_len > 0, "{:?}", layout.text);
    assert_eq!(
        layout_offsets(layout, node, 1),
        [marker_len, marker_len + 1]
    );
}

#[test]
fn display_contents_collapses_to_its_own_space() {
    // The map and Parley must agree on the space a `display: contents` wrapper collapses to.
    let doc = layout_doc(
        "<div id=t><span style='display:contents;text-transform:full-width'>a </span>\
         <img id=i style='width:1px;height:1px'></div>",
    );
    let layout = inline_layout(&doc);
    assert_eq!(layout.text, "ａ\u{3000}\u{FFFC}");
    let img = doc.query_selector("#i").unwrap().unwrap();
    assert_eq!(layout.offset_map.layout_range(img), Some(6..9));
}

#[test]
fn stylo_text_transform_conversion() {
    // Both the case and the `full-width` flag of Stylo's `text-transform` reach the map.
    let doc = layout_doc("<div id=t style='text-transform:uppercase full-width'>aßb</div>");
    let layout = inline_layout(&doc);
    let [node] = text_nodes(&doc)[..] else {
        panic!()
    };
    assert_eq!(layout.text, "ＡＳＳＢ");
    assert_eq!(layout_offsets(layout, node, 4), [0, 3, 9, 9, 12]);
}
