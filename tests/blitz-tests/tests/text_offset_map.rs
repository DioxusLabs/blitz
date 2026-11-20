//! The map between DOM offsets and layout text offsets that Blitz records while it collapses
//! white space and applies `text-transform`, and the white space it reports at line ends.

use blitz_dom::node::{Affinity, LineEndWhitespace, TextLayout};
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
fn collapsed_white_space_across_nodes() {
    let doc = layout_doc("<div id=t>  a  <b> b </b>  c  </div>");
    let layout = inline_layout(&doc);
    let [a, b, c] = text_nodes(&doc)[..] else {
        panic!()
    };
    assert_eq!(layout.text, "a b c");
    // "  a  " -> "a ": the leading run is removed, the trailing one kept.
    assert_eq!(layout_offsets(layout, a, 5), [0, 0, 0, 1, 2, 2]);
    // " b " -> "b": the leading space collapses into the pending one before it.
    assert_eq!(layout_offsets(layout, b, 3), [2, 2, 3, 4]);
    assert_eq!(layout_offsets(layout, c, 5), [4, 4, 4, 5, 5, 5]);
    let map = &layout.offset_map;
    assert_eq!(map.source_position(2, Affinity::Upstream), Some((a, 4)));
    assert_eq!(map.source_position(2, Affinity::Downstream), Some((b, 1)));
    assert_eq!(map.source_position(1, Affinity::Downstream), Some((a, 3)));
    assert_eq!(map.source_position(5, Affinity::Upstream), Some((c, 3)));
    assert_eq!(map.source_position(5, Affinity::Downstream), Some((c, 5)));
}

#[test]
fn text_transforms_that_change_length() {
    let doc = layout_doc("<div id=t style='text-transform:uppercase'>aßb</div>");
    let layout = inline_layout(&doc);
    let [node] = text_nodes(&doc)[..] else {
        panic!()
    };
    assert_eq!(layout.text, "ASSB");
    // `ß` is two bytes in both texts, but one character becomes two.
    assert_eq!(layout_offsets(layout, node, 4), [0, 1, 3, 3, 4]);
    let map = &layout.offset_map;
    assert_eq!(map.source_position(2, Affinity::Upstream), Some((node, 1)));
    assert_eq!(
        map.source_position(2, Affinity::Downstream),
        Some((node, 3))
    );
}

#[test]
fn full_width_collapses_to_ideographic_space() {
    let doc = layout_doc("<div id=t style='text-transform:full-width'>a  b</div>");
    let layout = inline_layout(&doc);
    let [node] = text_nodes(&doc)[..] else {
        panic!()
    };
    assert_eq!(layout.text, "ａ\u{3000}ｂ");
    assert_eq!(layout_offsets(layout, node, 4), [0, 3, 6, 6, 9]);
}

#[test]
fn breaks_and_inline_boxes() {
    let doc = layout_doc(
        "<div id=t style='white-space:pre-line'>a \n b<img style='width:1px;height:1px'> \
         <span style='float:left'>x</span> c<br>d</div>",
    );
    let layout = inline_layout(&doc);
    let nodes = text_nodes(&doc);
    // Spaces around the preserved break are removed, and the float doesn't end the run.
    assert_eq!(layout.text, "a\nb\u{FFFC} c\nd");
    assert_eq!(layout_offsets(layout, nodes[0], 5), [0, 1, 1, 2, 2, 3]);
    // " c" -> "c": its space collapsed into the one before the float.
    assert_eq!(layout_offsets(layout, nodes[3], 2), [7, 7, 8]);
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
fn line_end_white_space() {
    // Collapsible spaces at a line end are removed; preserved spaces that wrap hang.
    let doc = layout_doc("<div id=t style='width:0'>aaa bbb</div>");
    let layout = inline_layout(&doc);
    assert_eq!(
        layout.line_end_whitespace(0),
        Some(LineEndWhitespace {
            hanging: 3..3,
            removed: 3..4,
        })
    );
    let doc = layout_doc("<div id=t style='width:0; white-space:pre-wrap'>aaa  \nbbb   ccc</div>");
    let layout = inline_layout(&doc);
    assert_eq!(
        layout.line_end_whitespace(0),
        Some(LineEndWhitespace {
            hanging: 3..5,
            removed: 5..5,
        })
    );
    assert_eq!(
        layout.line_end_whitespace(1),
        Some(LineEndWhitespace {
            hanging: 9..12,
            removed: 12..12,
        })
    );
    // Preserved spaces that don't wrap don't hang.
    let doc = layout_doc("<div id=t style='width:0; white-space:pre'>aaa  \nbbb</div>");
    let layout = inline_layout(&doc);
    assert_eq!(
        layout.line_end_whitespace(0),
        Some(LineEndWhitespace {
            hanging: 5..5,
            removed: 5..5,
        })
    );
}
