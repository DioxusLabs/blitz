//! The floats an inline formatting context's lines flow around, as Taffy's block formatting
//! context places them.

use blitz_traits::node_id::NodeId;
use taffy::{BlockContext, LayoutInput, OofCandidates, Rect, Size};

use crate::layout::LayoutPassState;
use crate::text::{LineFloats, LineSlot};

/// The floats of the block formatting context an inline formatting context's lines are broken in,
/// in Taffy's terms: each float the lines reach is laid out and placed into it at once.
///
/// The lines are in device pixels and Taffy is in CSS pixels, so every answer is scaled on the way
/// through.
#[cfg_attr(not(feature = "floats"), allow(dead_code))]
pub(crate) struct TaffyFloats<'s, 'a, 'c, 'bfc> {
    pub(crate) state: &'s mut LayoutPassState<'a>,
    pub(crate) block_ctx: &'c mut BlockContext<'bfc>,
    /// Device pixels per CSS pixel.
    pub(crate) scale: f32,
    /// The inputs a float is laid out with.
    pub(crate) float_inputs: LayoutInput,
    /// What a float's margin percentages are of.
    pub(crate) parent_size: Size<Option<f32>>,
    /// The inline root's padding and border, which its content box is inside.
    pub(crate) container_pb: Rect<f32>,
    /// The absolutely positioned boxes the floats hold, as the inline root hands them on.
    pub(crate) oof_candidates: &'s mut OofCandidates,
}

#[cfg(feature = "floats")]
impl LineFloats for TaffyFloats<'_, '_, '_, '_> {
    fn slot(&self, y: f64) -> LineSlot {
        let scale = self.scale;
        let min_y = y / scale as f64;
        let slot = self
            .block_ctx
            .find_content_slot(min_y as f32, taffy::Clear::None, None);
        LineSlot {
            x: slot.x * scale,
            y: (slot.y * scale) as f64,
            width: slot.width * scale,
            narrowed: slot.segment_id.is_some(),
        }
    }

    fn place_float(&mut self, node_id: NodeId, y: f64) {
        use taffy::{BlockItemStyle as _, CoreStyle as _, Float, LayoutPartialTree as _};
        use taffy::{ResolveOrZero as _, prelude::TaffyMaxContent as _};

        use crate::layout::resolve_calc_value;

        let state = &mut *self.state;
        let (direction, clear, margin) = {
            let style = state.child_layout_style(&state.nodes[node_id]);
            // Only floats break the lines at their boxes.
            let direction = match style.float() {
                Float::Left => taffy::FloatDirection::Left,
                Float::Right => taffy::FloatDirection::Right,
                Float::None => unreachable!(),
            };
            (
                direction,
                style.clear(),
                style
                    .margin()
                    .resolve_or_zero(self.parent_size, resolve_calc_value),
            )
        };

        let margin_sum = margin.sum_axes();

        let mut output = state.compute_child_layout(
            crate::taffy_node_id(node_id),
            LayoutInput {
                available_space: Size::MAX_CONTENT,
                ..self.float_inputs
            },
        );
        let min_y = y as f32 / self.scale;

        // Note: `pos` is content-box relative
        let pos = self.block_ctx.place_floated_box(
            output.size + margin_sum,
            min_y,
            direction,
            clear,
            false,
        );

        let location = taffy::Point {
            x: pos.x + margin.left + self.container_pb.left,
            y: pos.y + margin.top + self.container_pb.top,
        };
        let layout = state.nodes[node_id].unrounded_layout_mut();
        layout.size = output.size;
        layout.location = location;
        layout.margin = margin;

        // Translate anchors from item-relative to container-relative
        // coordinates and collect candidates bubbled from the float's subtree
        if !output.oof_candidates.is_empty() {
            output.oof_candidates.translate(location);
            self.oof_candidates.append(&mut output.oof_candidates);
        }
    }
}

/// Without float layout, every line is as wide as the lines are broken at.
#[cfg(not(feature = "floats"))]
impl LineFloats for TaffyFloats<'_, '_, '_, '_> {
    fn slot(&self, y: f64) -> LineSlot {
        LineSlot {
            x: 0.0,
            y,
            width: f32::INFINITY,
            narrowed: false,
        }
    }

    fn place_float(&mut self, _node_id: NodeId, _y: f64) {}
}
