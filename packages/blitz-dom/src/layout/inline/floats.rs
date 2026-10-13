//! The floats an inline formatting context's lines flow around, as Taffy's block formatting
//! context places them.

use crate::text::{FloatClear, FloatRequest, FloatSide, LineExclusions, LineSlot, PlacedFloat};

/// Taffy's block formatting context, as an inline formatting context's lines read and place
/// floats in it. The lines measure in device pixels and Taffy in CSS pixels, so every answer is
/// scaled on the way through.
pub(crate) struct TaffyFloats<'c, 'bfc> {
    pub(crate) ctx: &'c mut taffy::BlockContext<'bfc>,
    /// Device pixels a CSS pixel.
    pub(crate) scale: f32,
}

impl LineExclusions for TaffyFloats<'_, '_> {
    fn slot(&self, top: f32) -> Option<LineSlot> {
        let slot = self
            .ctx
            .find_content_slot(top / self.scale, taffy::Clear::None, None);
        slot.segment_id.map(|_| LineSlot {
            top: slot.y * self.scale,
            left: slot.x * self.scale,
            right: (slot.x + slot.width) * self.scale,
        })
    }

    fn place(&mut self, float: FloatRequest) -> PlacedFloat {
        let scale = self.scale;
        let pos = self.ctx.place_floated_box(
            float.key,
            taffy::Size {
                width: float.inline_size / scale,
                height: float.block_size / scale,
            },
            float.block_start / scale,
            match float.side {
                FloatSide::Left => taffy::FloatDirection::Left,
                FloatSide::Right => taffy::FloatDirection::Right,
            },
            match float.clear {
                FloatClear::None => taffy::Clear::None,
                FloatClear::Left => taffy::Clear::Left,
                FloatClear::Right => taffy::Clear::Right,
                FloatClear::Both => taffy::Clear::Both,
            },
            false,
        );
        let (left, top) = (pos.x * scale, pos.y * scale);
        PlacedFloat {
            left,
            right: left + float.inline_size,
            top,
            bottom: top + float.block_size,
        }
    }
}
