//! The floats an inline formatting context's lines flow around.

// Without float layout, the lines are broken without asking about floats.
#![cfg_attr(not(feature = "floats"), allow(dead_code))]

use crate::text::{FloatRequest, FloatSide, LineExclusions, PlacedFloat};

/// A float placed while the lines were broken: its node and its margin box in device pixels,
/// relative to the content box.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Floated {
    /// The key the content holds it by.
    pub(crate) key: u64,
    pub(crate) side: FloatSide,
    pub(crate) left: f32,
    pub(crate) right: f32,
    pub(crate) top: f32,
    pub(crate) bottom: f32,
}

impl Floated {
    /// Whether it narrows what reaches across the block from `start` to `end`: a line box, or a
    /// float being placed. A line of no height is asked about where it stands.
    fn overlaps(&self, start: f32, end: f32) -> bool {
        self.bottom > self.top && self.top < end.max(start + f32::EPSILON) && self.bottom > start
    }
}

/// The room the block formatting context leaves an inline formatting context's lines, and the
/// floats placed as the text reaches them.
///
/// The floats the blocks before this one placed are read from Taffy's context, which cannot take
/// one back; the ones this block's text places are kept here, where a trial break the line breaker
/// takes back takes them back too, and are handed to Taffy once the lines are settled.
///
/// The lines measure in device pixels and Taffy in CSS pixels, so every answer is scaled on the
/// way through.
pub(crate) struct FloatRoom<'c, 'bfc> {
    #[cfg(feature = "floats")]
    pub(crate) outer: Option<&'c taffy::BlockContext<'bfc>>,
    #[cfg(not(feature = "floats"))]
    pub(crate) outer: core::marker::PhantomData<(&'c (), &'bfc ())>,
    /// How wide a line is where no float is in the way.
    pub(crate) width: f32,
    /// Device pixels a CSS pixel, which the floats of the blocks before this one are placed in.
    pub(crate) scale: f32,
    /// Each float's `clear`, by key.
    #[cfg(feature = "floats")]
    pub(crate) clears: &'c [(u64, taffy::Clear)],
    /// The floats placed so far, in the order the text reached them.
    pub(crate) placed: Vec<Floated>,
}

impl FloatRoom<'_, '_> {
    /// The room along a line that reaches across the block from `start` to `end`, as the floats
    /// before this block leave it.
    #[cfg_attr(not(feature = "floats"), allow(unused_mut))]
    fn outer_band(&self, start: f32, end: f32) -> (f32, f32) {
        let mut band = (0.0, self.width);
        #[cfg(feature = "floats")]
        if let Some(outer) = self.outer {
            // Every segment of floats the line reaches across: the narrowest of them is its room.
            let scale = self.scale;
            let mut at = start / scale;
            let mut after = None;
            for _ in 0..64 {
                let slot = outer.find_content_slot(at, taffy::Clear::None, after);
                let Some(segment) = slot.segment_id else {
                    break;
                };
                band.0 = f32::max(band.0, slot.x * scale);
                band.1 = f32::min(band.1, (slot.x + slot.width) * scale);
                let next = outer.find_content_slot(at, taffy::Clear::None, Some(segment));
                if next.segment_id.is_none() || next.y * scale >= end.max(start + f32::EPSILON) {
                    break;
                }
                at = next.y;
                after = Some(segment);
            }
        }
        #[cfg(not(feature = "floats"))]
        let _ = (start, end);
        band
    }

    /// The next position below `top` where the floats before this block change the room.
    fn outer_below(&self, top: f32) -> Option<f32> {
        #[cfg(feature = "floats")]
        if let Some(outer) = self.outer {
            let at = top / self.scale;
            let slot = outer.find_content_slot(at, taffy::Clear::None, None);
            let segment = slot.segment_id?;
            let next = outer.find_content_slot(at, taffy::Clear::None, Some(segment));
            if next.segment_id.is_some() {
                return Some(next.y * self.scale);
            }
            // Past the last segment no float is in the way.
            return outer
                .cleared_threshold(taffy::Clear::Both)
                .map(|bottom| bottom * self.scale)
                .filter(|bottom| *bottom > top);
        }
        #[cfg(not(feature = "floats"))]
        let _ = top;
        None
    }

    /// The room a line or a float has across the block from `start` to `end`, every float
    /// counted.
    fn room(&self, start: f32, end: f32) -> (f32, f32) {
        let mut band = self.outer_band(start, end);
        for float in &self.placed {
            if float.overlaps(start, end) {
                match float.side {
                    FloatSide::Left => band.0 = band.0.max(float.right),
                    FloatSide::Right => band.1 = band.1.min(float.left),
                }
            }
        }
        band
    }

    /// Where the floats placed on the sides the float `key` clears end: the top it is placed from.
    fn cleared(&self, key: u64) -> f32 {
        #[cfg(feature = "floats")]
        {
            let clear = self
                .clears
                .iter()
                .find(|(node, _)| *node == key)
                .map_or(taffy::Clear::None, |(_, clear)| *clear);
            let clears = |side: FloatSide| match clear {
                taffy::Clear::Both => true,
                taffy::Clear::Left => side == FloatSide::Left,
                taffy::Clear::Right => side == FloatSide::Right,
                taffy::Clear::None => false,
            };
            let local = self
                .placed
                .iter()
                .filter(|float| clears(float.side))
                .map(|float| float.bottom)
                .fold(f32::NEG_INFINITY, f32::max);
            let outer = self
                .outer
                .and_then(|outer| outer.cleared_threshold(clear))
                .map_or(f32::NEG_INFINITY, |bottom| bottom * self.scale);
            local.max(outer)
        }
        #[cfg(not(feature = "floats"))]
        {
            let _ = key;
            f32::NEG_INFINITY
        }
    }
}

impl LineExclusions for FloatRoom<'_, '_> {
    fn band(&self, start: f32, end: f32) -> (f32, f32) {
        self.room(start, end)
    }

    fn below(&self, top: f32) -> Option<f32> {
        let local = self
            .placed
            .iter()
            .flat_map(|float| [float.top, float.bottom])
            .filter(|edge| *edge > top)
            .fold(None, |least: Option<f32>, edge| {
                Some(least.map_or(edge, |least| least.min(edge)))
            });
        match (local, self.outer_below(top)) {
            (Some(local), Some(outer)) => Some(local.min(outer)),
            (local, outer) => local.or(outer),
        }
    }

    /// Places a float as CSS 2.1 §9.5.1 does: no higher than the line that reached it or any
    /// float before it, below what it clears, and as high as it fits, then as far to its side as
    /// it goes.
    fn place(&mut self, float: FloatRequest) -> PlacedFloat {
        let width = float.inline_size.max(0.0);
        let height = float.block_size.max(0.0);
        let earlier = self
            .placed
            .iter()
            .map(|placed| placed.top)
            .fold(f32::NEG_INFINITY, f32::max);
        let mut top = float.block_start.max(earlier).max(self.cleared(float.key));
        let mut band = self.room(top, top + height);
        for _ in 0..256 {
            // It fits, or nothing narrows the room it does not fit in.
            if band.1 - band.0 >= width - 0.01 || (band.0 <= 0.0 && band.1 >= self.width) {
                break;
            }
            let Some(next) = self.below(top).filter(|next| *next > top) else {
                break;
            };
            top = next;
            band = self.room(top, top + height);
        }
        let left = match float.side {
            FloatSide::Left => band.0,
            FloatSide::Right => band.1 - width,
        };
        self.placed.push(Floated {
            key: float.key,
            side: float.side,
            left,
            right: left + width,
            top,
            bottom: top + height,
        });
        PlacedFloat {
            left,
            right: left + width,
            top,
            bottom: top + height,
        }
    }

    fn checkpoint(&self) -> usize {
        self.placed.len()
    }

    fn rewind(&mut self, to: usize) {
        self.placed.truncate(to);
    }
}
