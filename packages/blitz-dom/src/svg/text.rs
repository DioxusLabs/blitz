//! `<text>` / `<tspan>` shaping via parley.
//!
//! Reuses the same `TreeBuilder<TextBrush>` + `stylo_to_parley::style` machinery the normal HTML inline-layout
//! path uses, just driven over an SVG `<text>`/`<tspan>` subtree instead of an HTML inline formatting context.

use blitz_traits::node_id::NodeId;
use parley::{FontContext, LayoutContext, TreeBuilder};

use crate::BaseDocument;
use crate::node::{NodeData, TextBrush};

use super::attrs::raw_attr;
use super::context::TextRun;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAnchor {
    #[default]
    Start,
    Middle,
    End,
}

fn parse_text_anchor(attrs: &[crate::node::Attribute]) -> TextAnchor {
    match raw_attr(attrs, "text-anchor") {
        Some("middle") => TextAnchor::Middle,
        Some("end") => TextAnchor::End,
        _ => TextAnchor::Start,
    }
}

/// Shape a `<text>` element into a parley `Layout`. `scale` is the document's device scale factor. Returns
/// `None` if the subtree has no styled root or no non-whitespace text.
pub fn shape_text(
    doc: &BaseDocument,
    text_root: NodeId,
    font_ctx: &mut FontContext,
    layout_ctx: &mut LayoutContext<TextBrush>,
    scale: f32,
) -> Option<TextRun> {
    let root_node = &doc.nodes[text_root];
    let root_style = root_node.primary_styles()?;
    let parley_style = crate::stylo_to_parley::style(text_root, &root_style);

    let mut builder = layout_ctx.tree_builder(font_ctx, scale, true, &parley_style);
    builder.set_white_space_mode(parley::WhiteSpaceCollapse::Collapse);

    let mut has_text = false;
    push_children(doc, text_root, &mut builder, &mut has_text);
    if !has_text {
        return None;
    }

    let mut layout = parley::Layout::default();
    builder.build_into(&mut layout);
    layout.break_all_lines(None);

    let anchor = parse_text_anchor(root_node.attrs().unwrap_or(&[]));
    Some(TextRun { layout, anchor })
}

fn push_children(
    doc: &BaseDocument,
    node_id: NodeId,
    builder: &mut TreeBuilder<TextBrush>,
    has_text: &mut bool,
) {
    let node = &doc.nodes[node_id];
    for &child_id in node.children.iter() {
        let child = &doc.nodes[child_id];
        match &child.data {
            NodeData::Text(text_data) => {
                let collapsed = collapse_whitespace(&text_data.content);
                if !collapsed.is_empty() {
                    builder.push_text(&collapsed);
                    *has_text = true;
                }
            }
            NodeData::Element(_) => {
                if let Some(style) = child.primary_styles() {
                    let child_style = crate::stylo_to_parley::style(child_id, &style);
                    builder.push_style_span(child_style);
                    push_children(doc, child_id, builder, has_text);
                    builder.pop_style_span();
                } else {
                    push_children(doc, child_id, builder, has_text);
                }
            }
            _ => {}
        }
    }
}

/// SVG default whitespace handling (`xml:space="default"`): collapse runs of ASCII whitespace
/// to a single space. Leading/trailing trimming is left to parley's own line-breaking,
/// so only internal collapsing happens here.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                out.push(' ');
            }
            last_was_space = true;
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out
}

/// Horizontal shift to apply to a shaped run's origin so it lands at the SVG `x`.
pub fn anchor_shift(anchor: TextAnchor, full_width: f32) -> f32 {
    match anchor {
        TextAnchor::Start => 0.0,
        TextAnchor::Middle => -full_width / 2.0,
        TextAnchor::End => -full_width,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_internal_whitespace_runs() {
        assert_eq!(collapse_whitespace("a   b\n\tc"), "a b c");
    }

    #[test]
    fn anchor_shift_start_is_zero() {
        assert_eq!(anchor_shift(TextAnchor::Start, 100.0), 0.0);
    }

    #[test]
    fn anchor_shift_middle_and_end() {
        assert_eq!(anchor_shift(TextAnchor::Middle, 100.0), -50.0);
        assert_eq!(anchor_shift(TextAnchor::End, 100.0), -100.0);
    }
}
