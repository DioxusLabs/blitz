//! CSS vertical writing modes (`writing-modes` feature).
//!
//! Taffy is a horizontal-tb engine: `width`/x is the inline axis and `height`/+y the block axis.
//! Blitz runs each box's layout algorithm in the box's own writing-mode *frame* and translates at
//! the boundary (see [`stylo_taffy::frame`] for the axis mapping):
//!
//! - Style getters: a box's own algorithm reads its style in its own frame
//!   (`get_core_container_style`); the algorithm laying it out reads it in *that* algorithm's frame
//!   (`get_*_child_style`, `get_oof_item_style`, [`BaseDocument::child_layout_style`]).
//!   [`BaseDocument::current_frame`] tracks the frame of the running algorithm.
//! - Orthogonal flows ([`compute_child_layout_in_frame`]): when a box's frame differs from its
//!   parent's, the `LayoutInput` is transposed into the box's frame, its auto inline size is
//!   resolved to fit-content (css-writing-modes-3 §7.3.1), and the `LayoutOutput` is transposed
//!   back. Percentage padding on such a box resolves against the containing block's inline size
//!   ([`BaseDocument::orthogonal_percent_basis`]).
//! - Geometry: `unrounded_layout` is written by the placing algorithm in *its* frame.
//!   [`physicalise_and_round_layout`] converts every layout to physical coordinates (mirroring
//!   the block axis for `*-rl` modes) and rounds it, replacing `taffy::round_layout`.
//!
//! [`compute_child_layout_in_frame`]: BaseDocument::compute_child_layout_in_frame
//! [`physicalise_and_round_layout`]: BaseDocument::physicalise_and_round_layout

use super::BlockContext;
use crate::document::BaseDocument;
use crate::dom_node_id;
use stylo_taffy::WritingMode;
use stylo_taffy::frame;
use taffy::{
    AvailableSpace, AxisStaticEdge, AxisStaticPosition, LayoutInput, LayoutOutput, NodeId, Point,
    RoundTree, RunMode, Size, TraversePartialTree,
};

impl BaseDocument {
    /// The writing-mode frame in which `node_id`'s layout algorithm runs.
    ///
    /// For the root element this applies the HTML special case of css-writing-modes-3 §8: when the
    /// root is `horizontal-tb`, the `writing-mode` of its `<body>` child is used instead.
    pub(crate) fn frame_of(&self, node_id: crate::NodeId) -> WritingMode {
        let node = &self.nodes[node_id];
        let frame = node.writing_mode();
        if frame.is_vertical() || node_id != self.root_element().id || has_containment(node) {
            return frame;
        }
        node.children
            .iter()
            .map(|&child| &self.nodes[child])
            .find(|child| {
                child
                    .data
                    .downcast_element()
                    .is_some_and(|el| *el.name.local == *"body")
            })
            .filter(|body| !has_containment(body))
            .map(|body| body.writing_mode())
            .unwrap_or(frame)
    }

    /// Lay out `node_id`, translating between the parent's frame and the node's own frame at an
    /// orthogonal-flow boundary.
    pub(crate) fn compute_child_layout_in_frame(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        let node_frame = self.frame_of(dom_node_id(node_id));
        let parent_frame = core::mem::replace(&mut self.current_frame, node_frame);
        let parent_align_axis_is_inline = core::mem::replace(
            &mut self.current_align_axis_is_inline,
            self.nodes[dom_node_id(node_id)].is_column_flex_container(),
        );

        let output = if node_frame.is_vertical() == parent_frame.is_vertical() {
            self.compute_child_layout_same_frame(node_id, inputs, block_ctx)
        } else {
            self.compute_orthogonal_child_layout(node_id, inputs, parent_frame)
        };

        self.current_frame = parent_frame;
        self.current_align_axis_is_inline = parent_align_axis_is_inline;
        output
    }

    fn compute_orthogonal_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        parent_frame: WritingMode,
    ) -> LayoutOutput {
        let percent_basis = self.orthogonal_percent_basis.replace((
            dom_node_id(node_id),
            inputs.parent_size.width.unwrap_or(0.0),
        ));

        let mut inputs = inputs.transpose();

        // A block container stretches its children's inline size and passes it as a known
        // dimension. For an orthogonal child that is its block size, which is content-sized.
        let node = &self.nodes[dom_node_id(node_id)];
        let placed_by_block_container = node
            .layout_parent
            .get()
            .map(|parent| self.nodes[parent].taffy_display() == taffy::Display::Block)
            .unwrap_or(true);
        if placed_by_block_container
            && taffy::CoreStyle::size(&node.layout_style())
                .height
                .is_auto()
        {
            inputs.known_dimensions.height = None;
            inputs.known_dimensions_are_definite.height = false;
        }

        if inputs.run_mode != RunMode::PerformHiddenLayout
            && inputs.known_dimensions.width.is_none()
        {
            let inline_size_is_auto =
                taffy::CoreStyle::size(&self.nodes[dom_node_id(node_id)].layout_style())
                    .width
                    .is_auto();
            if inline_size_is_auto {
                // css-writing-modes-3 §7.3.1: an auto inline size in an orthogonal flow is
                // fit-content against the available space in that axis: the containing block's
                // block size if definite, else the initial containing block (nearest fixed-size
                // scroll container not implemented).
                let available = match (inputs.available_space.width, inputs.parent_size.width) {
                    (AvailableSpace::Definite(size), _) => size,
                    (_, Some(size)) => size,
                    _ => {
                        let viewport = self.stylist.device().au_viewport_size();
                        if parent_frame.is_vertical() {
                            viewport.width.to_f32_px()
                        } else {
                            viewport.height.to_f32_px()
                        }
                    }
                };
                let measure = |doc: &mut Self, available_space: AvailableSpace| {
                    let measure_inputs = LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        axis: taffy::RequestedAxis::Horizontal,
                        known_dimensions: Size {
                            width: None,
                            height: inputs.known_dimensions.height,
                        },
                        available_space: Size {
                            width: available_space,
                            height: inputs.available_space.height,
                        },
                        ..inputs
                    };
                    doc.compute_child_layout_same_frame(node_id, measure_inputs, None)
                        .size
                        .width
                };
                let max_content = measure(self, AvailableSpace::MaxContent);
                let min_content = measure(self, AvailableSpace::MinContent);
                inputs.known_dimensions.width = Some(available.min(max_content).max(min_content));
                inputs.known_dimensions_are_definite.width = true;
            }
        }

        let mut output = self.compute_child_layout_same_frame(node_id, inputs, None);
        self.orthogonal_percent_basis = percent_basis;
        transpose_static_positions(&mut output, self.current_frame, parent_frame);
        output.transpose()
    }

    /// Convert every `unrounded_layout` (written in the frame of the box that placed it) to
    /// physical coordinates and round it into `final_layout`. Mirrors `taffy::round_layout`,
    /// which it replaces: rounding is applied to cumulative physical positions so that adjacent
    /// boxes never gain gaps or overlaps.
    pub(crate) fn physicalise_and_round_layout(&mut self, root: NodeId) {
        let viewport = self.stylist.device().au_viewport_size();
        let viewport = Size {
            width: viewport.width.to_f32_px(),
            height: viewport.height.to_f32_px(),
        };
        let root_frame = self.frame_of(dom_node_id(root));
        self.physicalise_and_round_inner(root, root_frame, viewport, Point::ZERO);
    }

    fn physicalise_and_round_inner(
        &mut self,
        node_id: NodeId,
        placer_frame: WritingMode,
        placer_size: Size<f32>,
        parent_pos: Point<f32>,
    ) {
        let logical = self.get_unrounded_layout(node_id);
        let mut unrounded = logical;
        if placer_frame.is_vertical() {
            unrounded.location = Point {
                x: if placer_frame.is_vertical_lr() {
                    logical.location.y
                } else {
                    placer_size.width - logical.location.y - logical.size.height
                },
                y: logical.location.x,
            };
            unrounded.size = logical.size.transpose();
            unrounded.scrollbar_size = logical.scrollbar_size.transpose();
            unrounded.scrollable_overflow_rect = logical.scrollable_overflow_rect.transpose();
            unrounded.border = frame::rect_to_physical(placer_frame, logical.border);
            unrounded.padding = frame::rect_to_physical(placer_frame, logical.padding);
            unrounded.margin = frame::rect_to_physical(placer_frame, logical.margin);
        }

        let round = |value: f32| value.round();
        let pos = Point {
            x: parent_pos.x + unrounded.location.x,
            y: parent_pos.y + unrounded.location.y,
        };
        let size = unrounded.size;
        let mut layout = unrounded;
        layout.location.x = round(pos.x) - round(parent_pos.x);
        layout.location.y = round(pos.y) - round(parent_pos.y);
        layout.size.width = round(pos.x + size.width) - round(pos.x);
        layout.size.height = round(pos.y + size.height) - round(pos.y);
        layout.scrollbar_size.width = round(unrounded.scrollbar_size.width);
        layout.scrollbar_size.height = round(unrounded.scrollbar_size.height);
        layout.border.left = round(pos.x + unrounded.border.left) - round(pos.x);
        layout.border.right =
            round(pos.x + size.width) - round(pos.x + size.width - unrounded.border.right);
        layout.border.top = round(pos.y + unrounded.border.top) - round(pos.y);
        layout.border.bottom =
            round(pos.y + size.height) - round(pos.y + size.height - unrounded.border.bottom);
        layout.padding.left = round(pos.x + unrounded.padding.left) - round(pos.x);
        layout.padding.right =
            round(pos.x + size.width) - round(pos.x + size.width - unrounded.padding.right);
        layout.padding.top = round(pos.y + unrounded.padding.top) - round(pos.y);
        layout.padding.bottom =
            round(pos.y + size.height) - round(pos.y + size.height - unrounded.padding.bottom);
        let overflow = unrounded.scrollable_overflow_rect;
        layout.scrollable_overflow_rect.left = round(pos.x + overflow.left) - round(pos.x);
        layout.scrollable_overflow_rect.right = round(pos.x + overflow.right) - round(pos.x);
        layout.scrollable_overflow_rect.top = round(pos.y + overflow.top) - round(pos.y);
        layout.scrollable_overflow_rect.bottom = round(pos.y + overflow.bottom) - round(pos.y);

        // `unrounded_layout` is left in the placer's frame: Taffy's cache may skip re-placing
        // this node on a later layout, and only `final_layout` is read downstream.
        self.set_final_layout(node_id, &layout);

        let frame = self.frame_of(dom_node_id(node_id));
        for index in 0..self.child_count(node_id) {
            let child = self.get_child_id(node_id, index);
            if !self.is_out_of_flow(child) {
                self.physicalise_and_round_inner(child, frame, size, pos);
            }
        }
        for index in 0..self.hoisted_child_count(node_id) {
            let child = self.get_hoisted_child_id(node_id, index);
            self.physicalise_and_round_inner(child, frame, size, pos);
        }
    }
}

/// Map the static positions of `output`'s out-of-flow candidates (relative to the border box of a
/// node laid out in `child_frame`) into `parent_frame`, ahead of [`LayoutOutput::transpose`],
/// which leaves them untouched. The block axis of a `vertical-rl` frame runs against Taffy's
/// coordinate, so that coordinate is mirrored within the node's size.
fn transpose_static_positions(
    output: &mut LayoutOutput,
    child_frame: WritingMode,
    parent_frame: WritingMode,
) {
    if output.oof_candidates.is_empty() {
        return;
    }
    let vertical_frame = if child_frame.is_vertical() {
        child_frame
    } else {
        parent_frame
    };
    let mirror_block = !vertical_frame.is_vertical_lr();
    let size = output.size;
    for candidate in output.oof_candidates.as_mut_slice() {
        let Point { mut x, mut y } = candidate.static_position;
        if mirror_block {
            if child_frame.is_vertical() {
                y = mirror_static_position(y, size.height);
            } else {
                x = mirror_static_position(x, size.width);
            }
        }
        candidate.static_position = Point { x: y, y: x };
    }
}

fn mirror_static_position(position: AxisStaticPosition, extent: f32) -> AxisStaticPosition {
    let flip = |edge: AxisStaticEdge| match edge {
        AxisStaticEdge::Start => AxisStaticEdge::End,
        AxisStaticEdge::End => AxisStaticEdge::Start,
        AxisStaticEdge::Center => AxisStaticEdge::Center,
    };
    AxisStaticPosition {
        area: taffy::Line {
            start: extent - position.area.end,
            end: extent - position.area.start,
        },
        align: taffy::AxisStaticAlign {
            keyword: flip(position.align.keyword),
            fallback: flip(position.align.fallback),
            ..position.align
        },
    }
}

/// Any `contain` value on `<html>` or `<body>` disables body-to-root `writing-mode` propagation.
fn has_containment(node: &crate::Node) -> bool {
    node.primary_styles()
        .is_some_and(|style| !style.clone_contain().is_empty())
}
