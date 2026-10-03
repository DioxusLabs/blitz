//! Box geometry for non-atomic inline elements (e.g. `<span>`).
//!
//! Such elements have no Taffy layout box. They are laid out as style spans in their inline
//! root's Parley layout, which only knows about the inline-axis size of their edges (the sum
//! of the margin, border and padding on a side). Everything else about their boxes is
//! resolved here.

use parley::SpanFragment;
use taffy::{CoreStyle as _, ResolveOrZero as _};

use super::resolve_calc_value;
use crate::node::Node;
use crate::{BaseDocument, NodeId};

/// The resolved margin, border and padding of a non-atomic inline element, in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InlineSpanBox {
    pub(crate) margin: taffy::Rect<f32>,
    pub(crate) border: taffy::Rect<f32>,
    pub(crate) padding: taffy::Rect<f32>,
}

impl InlineSpanBox {
    /// Resolves the box of `node`. Percentages on all four sides resolve against the width of
    /// the containing block (`percent_basis`), or to zero if it is not known. `auto` margins
    /// are zero.
    pub(crate) fn resolve(node: &Node, percent_basis: Option<f32>) -> Option<Self> {
        node.primary_styles()?;
        let style = node.layout_style();
        // Unlike a size, a percentage margin or padding with no basis behaves as zero, even
        // within a `calc()`.
        let percent_basis = Some(percent_basis.unwrap_or(0.0));
        Some(Self {
            margin: style
                .margin()
                .resolve_or_zero(percent_basis, resolve_calc_value),
            border: style
                .border()
                .resolve_or_zero(percent_basis, resolve_calc_value),
            padding: style
                .padding()
                .resolve_or_zero(percent_basis, resolve_calc_value),
        })
    }

    /// The `(inline_start, inline_end)` edges to give to Parley, in CSS pixels.
    ///
    /// Stylo has already mapped logical properties (e.g. `padding-inline-start`) onto the
    /// physical sides, so this is the only place a physical side is mapped to a logical one.
    pub(crate) fn inline_edges(&self, is_rtl: bool) -> (f32, f32) {
        let left = self.margin.left + self.border.left + self.padding.left;
        let right = self.margin.right + self.border.right + self.padding.right;
        if is_rtl { (right, left) } else { (left, right) }
    }

    /// Whether the box takes up any space in the inline axis for some containing block width.
    pub(crate) fn has_inline_edges(node: &Node) -> bool {
        // Percentages resolve to zero without a basis, so also try with one.
        [None, Some(100.0)].into_iter().any(|percent_basis| {
            Self::resolve(node, percent_basis)
                .is_some_and(|span_box| span_box.inline_edges(false) != (0.0, 0.0))
        })
    }
}

/// The part of a non-atomic inline element's box that is on one line.
///
/// All values are in the coordinate space of the inline root's text layout: relative to the
/// inline root's content box and multiplied by the viewport scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InlineSpanFragment {
    pub node_id: NodeId,
    /// The index of the line the fragment is on
    pub line_index: usize,
    /// The left side of the border box
    pub x0: f32,
    /// The top of the border box
    pub y0: f32,
    /// The right side of the border box
    pub x1: f32,
    /// The bottom of the border box
    pub y1: f32,
    /// The border widths. The left and right borders are zero if that side of the box is on
    /// another line.
    pub border: taffy::Rect<f32>,
    /// The padding widths. The left and right padding are zero if that side of the box is on
    /// another line.
    pub padding: taffy::Rect<f32>,
    /// Whether the fragment has the left side of the box
    pub has_left_side: bool,
    /// Whether the fragment has the right side of the box
    pub has_right_side: bool,
}

impl BaseDocument {
    /// The width that percentage margins and padding of the inline-level descendants of
    /// `inline_root` resolve against.
    fn inline_percent_basis(inline_root: &Node) -> f32 {
        let layout = inline_root.unrounded_layout();
        layout.size.width
            - layout.padding.left
            - layout.padding.right
            - layout.border.left
            - layout.border.right
            - layout.scrollbar_size.width
    }

    /// The fragments of the non-atomic inline elements in `inline_root`'s text layout, line by
    /// line. Within a line, a fragment always comes after those of the element's ancestors.
    ///
    /// The block-axis extent of a fragment is that of the element's font, plus its vertical
    /// padding and border. It does not depend on the height of the line.
    pub fn inline_span_fragments<'a>(
        &'a self,
        inline_root: &'a Node,
    ) -> impl Iterator<Item = InlineSpanFragment> + 'a {
        let text_layout = inline_root
            .element_data()
            .and_then(|element| element.inline_layout_data.as_ref());
        let percent_basis = Self::inline_percent_basis(inline_root);
        text_layout.into_iter().flat_map(move |text_layout| {
            let layout = &text_layout.layout;
            let scale = layout.scale();
            // An inline root with no content generates no line boxes (see
            // `compute_inline_layout`): the (empty) inline elements in it have no height.
            let has_line_boxes = !text_layout.text.is_empty()
                || layout.inline_boxes().len() > 0
                || layout.lines().any(|line| {
                    line.span_fragments().any(|fragment| {
                        let node_id = layout.styles()[fragment.style_index as usize].brush.id;
                        self.get_node(node_id)
                            .is_some_and(InlineSpanBox::has_inline_edges)
                    })
                });
            layout
                .lines()
                .enumerate()
                .flat_map(move |(line_index, line)| {
                    line.span_fragments().filter_map(move |fragment| {
                        let node_id = layout.styles()[fragment.style_index as usize].brush.id;
                        if node_id == inline_root.id {
                            return None;
                        }
                        let node = self.get_node(node_id)?;
                        let span_box = InlineSpanBox::resolve(node, Some(percent_basis))?;
                        let mut fragment = fragment;
                        if !has_line_boxes {
                            (fragment.baseline, fragment.ascent, fragment.descent) =
                                (0.0, 0.0, 0.0);
                        }
                        Some(span_fragment(
                            node_id, line_index, &fragment, &span_box, scale,
                        ))
                    })
                })
        })
    }
}

fn span_fragment(
    node_id: NodeId,
    line_index: usize,
    fragment: &SpanFragment,
    span_box: &InlineSpanBox,
    scale: f32,
) -> InlineSpanFragment {
    let mut margin = span_box.margin.map(|v| v * scale);
    let mut border = span_box.border.map(|v| v * scale);
    let mut padding = span_box.padding.map(|v| v * scale);

    // Parley places the edges in visual order, in the direction of the inline root.
    let (has_left_side, has_right_side) = if fragment.is_rtl {
        (fragment.has_end_edge, fragment.has_start_edge)
    } else {
        (fragment.has_start_edge, fragment.has_end_edge)
    };
    if !has_left_side {
        (margin.left, border.left, padding.left) = (0.0, 0.0, 0.0);
    }
    if !has_right_side {
        (margin.right, border.right, padding.right) = (0.0, 0.0, 0.0);
    }

    InlineSpanFragment {
        node_id,
        line_index,
        x0: fragment.x + margin.left,
        y0: fragment.baseline - fragment.ascent - padding.top - border.top,
        x1: fragment.x + fragment.advance - margin.right,
        y1: fragment.baseline + fragment.descent + padding.bottom + border.bottom,
        border,
        padding,
        has_left_side,
        has_right_side,
    }
}
