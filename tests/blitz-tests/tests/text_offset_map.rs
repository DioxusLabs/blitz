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
fn composed_text_transforms() {
    let doc = layout_doc("<div id=t style='text-transform:uppercase full-width'>aßb</div>");
    let layout = inline_layout(&doc);
    let [node] = text_nodes(&doc)[..] else {
        panic!()
    };
    assert_eq!(layout.text, "ＡＳＳＢ");
    assert_eq!(layout_offsets(layout, node, 4), [0, 3, 9, 9, 12]);

    // Greek uppercasing removes the accent and `ß` expands, so the character count is unchanged.
    let doc = layout_doc("<div id=t lang=el style='text-transform:uppercase'>α\u{301}ßb</div>");
    let layout = inline_layout(&doc);
    let [node] = text_nodes(&doc)[..] else {
        panic!()
    };
    assert_eq!(layout.text, "ΑSSB");
    assert_eq!(layout_offsets(layout, node, 7), [0, 1, 2, 2, 2, 4, 4, 5]);
}

#[test]
fn out_of_flow_boxes_follow_pending_space() {
    // The map keeps DOM order, so the float comes after the space before it. Parley anchors it
    // before that space.
    let doc = layout_doc("<div id=t>a <span id=f style='float:left'>x</span>b</div>");
    let layout = inline_layout(&doc);
    assert_eq!(layout.text, "a b");
    let float = doc.query_selector("#f").unwrap().unwrap();
    assert_eq!(layout.offset_map.layout_range(float), Some(2..2));
    let anchor = layout
        .layout
        .inline_boxes()
        .find(|inline_box| inline_box.id == float.as_u64())
        .unwrap()
        .index;
    assert_eq!(anchor, 1);
}

#[test]
fn line_end_white_space_follows_layout() {
    // Before a forced break, preserved spaces only hang if they overflow.
    let doc = layout_doc("<div id=t style='white-space:pre-wrap'>aaa  \nbbb</div>");
    let layout = inline_layout(&doc);
    assert_eq!(
        layout.line_end_whitespace(0),
        Some(LineEndWhitespace {
            hanging: 5..5,
            removed: 5..5,
        })
    );
    // A preserved U+3000 from `full-width` hangs too.
    let doc = layout_doc(
        "<div id=t style='width:0; white-space:pre-wrap; text-transform:full-width'>a b</div>",
    );
    let layout = inline_layout(&doc);
    assert_eq!(layout.text, "ａ\u{3000}ｂ");
    assert_eq!(
        layout.line_end_whitespace(0),
        Some(LineEndWhitespace {
            hanging: 3..6,
            removed: 6..6,
        })
    );
}
