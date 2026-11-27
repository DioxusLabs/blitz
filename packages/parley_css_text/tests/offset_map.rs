//! White space collapsing, `text-transform` and the offset map, driven through Parley's
//! `StyleRunBuilder` the way a document engine would.

use parley::{
    FontContext, InlineBox, Layout, LayoutContext, TextStyle, TextWrapMode, WhiteSpaceCollapse,
};
use parley_css_text::{
    Affinity, CaseTransform, LineEndWhitespace, TextCase, TextOffsetMap, TextTransform,
    TextTransformer, WhiteSpace, WhiteSpaceCollapser, line_end_whitespace,
};

/// The computed styles of a text node that the collapser and transformer read.
#[derive(Clone, Copy)]
struct Style {
    collapse: WhiteSpaceCollapse,
    wrap: TextWrapMode,
    transform: TextTransform,
    lang: &'static str,
}

const NORMAL: Style = Style {
    collapse: WhiteSpaceCollapse::Collapse,
    wrap: TextWrapMode::Wrap,
    transform: TextTransform::NONE,
    lang: "",
};

const fn transformed(transform: TextTransform) -> Style {
    Style {
        transform,
        ..NORMAL
    }
}

const fn white_space(collapse: WhiteSpaceCollapse, wrap: TextWrapMode) -> Style {
    Style {
        collapse,
        wrap,
        ..NORMAL
    }
}

const fn case(case: TextCase) -> TextTransform {
    TextTransform {
        case,
        ..TextTransform::NONE
    }
}

const FULL_WIDTH: TextTransform = TextTransform {
    full_width: true,
    ..TextTransform::NONE
};

/// An inline formatting context's content, in tree order. Node ids are `u32`s.
enum Item {
    Text(u32, &'static str, Style),
    InlineBox(u32),
    OutOfFlowBox(u32),
    LineBreak(u32),
}

struct Laid {
    text: String,
    layout: Layout<()>,
    map: TextOffsetMap<u32>,
    out_of_flow_anchors: Vec<(u32, usize)>,
}

impl Laid {
    /// The layout offset of every byte offset of text node `node`.
    fn layout_offsets(&self, node: u32, len: usize) -> Vec<usize> {
        (0..=len)
            .map(|source| self.map.layout_offset(node, source).unwrap())
            .collect()
    }

    fn line_end_whitespace(&self, line: usize) -> LineEndWhitespace {
        let line = self.layout.get(line).unwrap();
        line_end_whitespace(&self.text, &self.layout, &line, &self.map)
    }
}

/// Builds `items` and breaks its lines at `width`.
fn lay_out(items: &[Item], width: Option<f32>) -> Laid {
    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();
    let mut builder = layout_cx.style_run_builder(&mut font_cx, 1.0, true);
    let root_style = builder.push_style(
        TextStyle {
            white_space_collapse: WhiteSpaceCollapse::Collapse,
            ..TextStyle::default()
        },
        None,
    );
    let mut collapser = WhiteSpaceCollapser::default();
    let mut transformer = TextTransformer::default();
    let mut out_of_flow_anchors = Vec::new();
    for item in items {
        match *item {
            Item::Text(node, source, style) => {
                let style_index = builder.push_style(
                    TextStyle {
                        white_space_collapse: style.collapse,
                        text_wrap_mode: style.wrap,
                        ..TextStyle::default()
                    },
                    Some(root_style),
                );
                let transform = CaseTransform::new(style.transform, style.collapse, style.lang);
                let (text, changes) = transformer.transform(
                    source,
                    &transform,
                    collapser.text(),
                    collapser.has_pending_whitespace(),
                );
                let white_space = WhiteSpace {
                    collapse: style.collapse,
                    collapsed_space: if style.transform.full_width {
                        "\u{3000}"
                    } else {
                        " "
                    },
                    wrap: style.wrap == TextWrapMode::Wrap,
                };
                collapser.push_text(
                    &mut builder,
                    style_index,
                    Some(root_style),
                    node,
                    source,
                    text,
                    changes,
                    white_space,
                );
            }
            Item::InlineBox(node) => {
                collapser.push_inline_box(
                    &mut builder,
                    root_style,
                    node,
                    InlineBox {
                        id: node.into(),
                        index: 0,
                        width: 1.0,
                        height: 1.0,
                        baseline: None,
                        vertical_align: Default::default(),
                    },
                );
                transformer.word_break(collapser.text().len());
            }
            Item::OutOfFlowBox(node) => {
                out_of_flow_anchors.push((node, collapser.push_out_of_flow_box(node)));
            }
            Item::LineBreak(node) => {
                collapser.push_line_break(&mut builder, root_style, node);
                transformer.word_break(collapser.text().len());
            }
        }
    }
    let (text, map) = collapser.finish();
    let mut layout = builder.build(&text);
    layout.break_all_lines(width);
    Laid {
        text,
        layout,
        map,
        out_of_flow_anchors,
    }
}

use Item::{InlineBox as Box, LineBreak, OutOfFlowBox, Text};

#[test]
fn collapsed_white_space_across_nodes() {
    // `  a  <b> b </b>  c  `
    let laid = lay_out(
        &[
            Text(1, "  a  ", NORMAL),
            Text(2, " b ", NORMAL),
            Text(3, "  c  ", NORMAL),
        ],
        None,
    );
    assert_eq!(laid.text, "a b c");
    // "  a  " -> "a ": the leading run is removed, the trailing one kept.
    assert_eq!(laid.layout_offsets(1, 5), [0, 0, 0, 1, 2, 2]);
    // " b " -> "b": the leading space collapses into the pending one before it.
    assert_eq!(laid.layout_offsets(2, 3), [2, 2, 3, 4]);
    assert_eq!(laid.layout_offsets(3, 5), [4, 4, 4, 5, 5, 5]);
    let map = &laid.map;
    assert_eq!(map.source_position(2, Affinity::Upstream), Some((1, 4)));
    assert_eq!(map.source_position(2, Affinity::Downstream), Some((2, 1)));
    assert_eq!(map.source_position(1, Affinity::Downstream), Some((1, 3)));
    assert_eq!(map.source_position(5, Affinity::Upstream), Some((3, 3)));
    assert_eq!(map.source_position(5, Affinity::Downstream), Some((3, 5)));
}

#[test]
fn text_transforms_that_change_length() {
    let laid = lay_out(
        &[Text(1, "aßb", transformed(case(TextCase::Uppercase)))],
        None,
    );
    assert_eq!(laid.text, "ASSB");
    // `ß` is two bytes in both texts, but one character becomes two.
    assert_eq!(laid.layout_offsets(1, 4), [0, 1, 3, 3, 4]);
    assert_eq!(
        laid.map.source_position(2, Affinity::Upstream),
        Some((1, 1))
    );
    assert_eq!(
        laid.map.source_position(2, Affinity::Downstream),
        Some((1, 3))
    );
}

#[test]
fn composed_text_transforms() {
    let uppercase_full_width = TextTransform {
        full_width: true,
        ..case(TextCase::Uppercase)
    };
    let laid = lay_out(&[Text(1, "aßb", transformed(uppercase_full_width))], None);
    assert_eq!(laid.text, "ＡＳＳＢ");
    assert_eq!(laid.layout_offsets(1, 4), [0, 3, 9, 9, 12]);
}

#[cfg(feature = "text-transform-icu")]
#[test]
fn language_sensitive_text_transforms() {
    // Greek uppercasing removes the accent and `ß` expands, so the character count is unchanged.
    let greek = Style {
        lang: "el",
        ..transformed(case(TextCase::Uppercase))
    };
    let laid = lay_out(&[Text(1, "α\u{301}ßb", greek)], None);
    assert_eq!(laid.text, "ΑSSB");
    assert_eq!(laid.layout_offsets(1, 7), [0, 1, 2, 2, 2, 4, 4, 5]);
}

#[test]
fn full_width_collapses_to_ideographic_space() {
    let laid = lay_out(&[Text(1, "a  b", transformed(FULL_WIDTH))], None);
    assert_eq!(laid.text, "ａ\u{3000}ｂ");
    assert_eq!(laid.layout_offsets(1, 4), [0, 3, 6, 6, 9]);
}

#[test]
fn breaks_and_inline_boxes() {
    // `a \n b<img> <float> c<br>d` under `white-space: pre-line`.
    let pre_line = white_space(WhiteSpaceCollapse::PreserveBreaks, TextWrapMode::Wrap);
    let laid = lay_out(
        &[
            Text(1, "a \n b", pre_line),
            Box(2),
            Text(3, " ", pre_line),
            OutOfFlowBox(4),
            Text(5, " c", pre_line),
            LineBreak(6),
            Text(7, "d", pre_line),
        ],
        None,
    );
    // Spaces around the preserved break are removed, and the float doesn't end the run.
    assert_eq!(laid.text, "a\nb\u{FFFC} c\nd");
    assert_eq!(laid.layout_offsets(1, 5), [0, 1, 1, 2, 2, 3]);
    // " c" -> "c": its space collapsed into the one before the float.
    assert_eq!(laid.layout_offsets(5, 2), [7, 7, 8]);
    assert_eq!(laid.map.layout_range(6), Some(8..9));
}

#[test]
fn out_of_flow_boxes_follow_pending_space() {
    // The map keeps document order, so the float comes after the space before it. Its Parley
    // anchor offset is before that space.
    let laid = lay_out(
        &[Text(1, "a ", NORMAL), OutOfFlowBox(2), Text(3, "b", NORMAL)],
        None,
    );
    assert_eq!(laid.text, "a b");
    assert_eq!(laid.map.layout_range(2), Some(2..2));
    let anchor = laid
        .out_of_flow_anchors
        .iter()
        .find(|&&(id, _)| id == 2)
        .unwrap()
        .1;
    assert_eq!(anchor, 1);
}

#[test]
fn line_end_white_space() {
    // Collapsible spaces at a line end are removed; preserved spaces that wrap hang.
    let laid = lay_out(&[Text(1, "aaa bbb", NORMAL)], Some(0.0));
    assert_eq!(
        laid.line_end_whitespace(0),
        LineEndWhitespace {
            hanging: 3..3,
            removed: 3..4,
        }
    );
    let pre_wrap = white_space(WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap);
    let laid = lay_out(&[Text(1, "aaa  \nbbb   ccc", pre_wrap)], Some(0.0));
    assert_eq!(
        laid.line_end_whitespace(0),
        LineEndWhitespace {
            hanging: 3..5,
            removed: 5..5,
        }
    );
    assert_eq!(
        laid.line_end_whitespace(1),
        LineEndWhitespace {
            hanging: 9..12,
            removed: 12..12,
        }
    );
    // Preserved spaces that don't wrap don't hang.
    let pre = white_space(WhiteSpaceCollapse::Preserve, TextWrapMode::NoWrap);
    let laid = lay_out(&[Text(1, "aaa  \nbbb", pre)], Some(0.0));
    assert_eq!(
        laid.line_end_whitespace(0),
        LineEndWhitespace {
            hanging: 5..5,
            removed: 5..5,
        }
    );
}

#[test]
fn line_end_white_space_follows_layout() {
    // Before a forced break, preserved spaces only hang if they overflow.
    let pre_wrap = white_space(WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap);
    let laid = lay_out(&[Text(1, "aaa  \nbbb", pre_wrap)], None);
    assert_eq!(
        laid.line_end_whitespace(0),
        LineEndWhitespace {
            hanging: 5..5,
            removed: 5..5,
        }
    );
    // A preserved U+3000 from `full-width` hangs too.
    let style = Style {
        transform: FULL_WIDTH,
        ..pre_wrap
    };
    let laid = lay_out(&[Text(1, "a b", style)], Some(0.0));
    assert_eq!(laid.text, "ａ\u{3000}ｂ");
    assert_eq!(
        laid.line_end_whitespace(0),
        LineEndWhitespace {
            hanging: 3..6,
            removed: 6..6,
        }
    );
}
