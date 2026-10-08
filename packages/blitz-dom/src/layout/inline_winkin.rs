//! Laying out an inline formatting context with winkin, in place of Parley.

use blitz_traits::node_id::NodeId;
use style::shared_lock::StylesheetGuards;
use style::values::specified::box_::BaselineSource;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
#[cfg(feature = "floats")]
use taffy::BlockItemStyle as _;
use taffy::{
    AvailableSpace, AxisStaticPosition, BlockContext, CollapsibleMarginSet, CoreStyle as _,
    Direction, LayoutInput, LayoutOutput, LayoutPartialTree as _, MaybeMath as _,
    MaybeResolve as _, OofCandidate, OofCandidates, OofPositioningArea, Point, Rect, RequestedAxis,
    ResolveOrZero as _, RunMode, Size, SizingMode,
};
use winkin::style::{Direction as TextDirection, WritingMode};
use winkin::{
    Area, BlockExtents, BoxSize, Exclusions, ExclusionsCheckpoint, FloatRequest, FloatSide,
    InlineExtents, Item, PlacedFloat,
};

use super::inline::{Frame, f32_max, inline_box_inputs};
use super::replaced::is_replaced_element;
use super::resolve_calc_value;
use crate::layout::LayoutPassState;
use crate::node::TextLayout;
use crate::text_winkin;

/// An atomic inline's margin box on its line, or where an absolutely
/// positioned box's static position is, in device pixels relative to the
/// content box.
struct Placed {
    node: NodeId,
    x: f32,
    top: f32,
    line_top: f32,
    line_bottom: f32,
    /// Whether an absolutely positioned box's static position faces right
    /// to left across the page: its inline start's way in a horizontal
    /// block, as winkin found it, and its block start's in a vertical one.
    /// `None` for an atomic inline.
    rtl: Option<bool>,
}

/// A float, or the block's initial letter, placed while the lines were
/// broken: its margin box in device pixels, relative to the content box.
#[derive(Copy, Clone, Debug)]
struct Floated {
    /// Its node, or the block's initial letter's key. Read to place the
    /// float's node, which only the float layout does.
    #[cfg_attr(not(feature = "floats"), allow(dead_code))]
    key: u64,
    side: FloatSide,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

impl Floated {
    /// Whether it narrows what reaches across the block from `start` to
    /// `end`: a line box, or a float being placed. A line of no height is
    /// asked about where it stands.
    fn overlaps(&self, start: f32, end: f32) -> bool {
        self.bottom > self.top && self.top < end.max(start + f32::EPSILON) && self.bottom > start
    }
}

/// The room the block formatting context leaves the lines, and the floats
/// placed as the text reaches them.
///
/// The floats the blocks before this one placed are read from taffy's
/// context, which cannot take one back; the ones this block's text places
/// are kept here, where a trial break the breaker takes back takes them
/// back too, and are handed to taffy once the lines are settled.
///
/// winkin measures in device pixels and taffy in CSS pixels, so every answer
/// is scaled on the way through.
struct FloatRoom<'c, 'bfc> {
    #[cfg(feature = "floats")]
    outer: Option<&'c BlockContext<'bfc>>,
    #[cfg(not(feature = "floats"))]
    outer: core::marker::PhantomData<(&'c (), &'bfc ())>,
    /// How wide a line is where no float is in the way.
    width: f32,
    /// Device pixels a CSS pixel, which the floats of the blocks before
    /// this one are placed in.
    #[cfg_attr(not(feature = "floats"), allow(dead_code))]
    scale: f32,
    /// Each float's `clear`, by node.
    #[cfg(feature = "floats")]
    clears: &'c [(u64, taffy::Clear)],
    /// The floats placed so far, in the order the text reached them.
    placed: Vec<Floated>,
}

impl FloatRoom<'_, '_> {
    /// The room along a line that reaches across the block from `start` to
    /// `end`, as the floats before this block leave it.
    #[cfg_attr(not(feature = "floats"), allow(unused_mut))]
    fn outer_band(&self, start: f32, end: f32) -> (f32, f32) {
        let mut band = (0.0, self.width);
        #[cfg(feature = "floats")]
        if let Some(outer) = self.outer {
            // Every segment of floats the line reaches across: the narrowest
            // of them is its room.
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

    /// The next position below `top` where the floats before this block
    /// change the room.
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

    /// The room a line or a float has across the block from `start` to
    /// `end`, every float counted.
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

    /// Where the floats placed on `side` or both, as `clear` asks, end: the
    /// top a float that clears them is placed from.
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

impl Exclusions for FloatRoom<'_, '_> {
    fn band(&self, _line: usize, block: BlockExtents) -> InlineExtents {
        let (left, right) = self.room(block.start, block.end);
        InlineExtents { left, right }
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

    /// Places a float as CSS 2.1 §9.5.1 does: no higher than the line that
    /// reached it or any float before it, below what it clears, and as high
    /// as it fits, then as far to its side as it goes.
    fn place(&mut self, float: FloatRequest) -> PlacedFloat {
        let width = float.inline_size.max(0.0);
        let height = float.block_size.max(0.0);
        let earlier = self
            .placed
            .iter()
            .map(|placed| placed.top)
            .fold(f32::NEG_INFINITY, f32::max);
        let mut top = float
            .block_start
            .max(earlier)
            .max(self.cleared(float.key.0));
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
            key: float.key.0,
            side: float.side,
            left,
            right: left + width,
            top,
            bottom: top + height,
        });
        PlacedFloat {
            inline: InlineExtents {
                left,
                right: left + width,
            },
            block: BlockExtents {
                start: top,
                end: top + height,
            },
        }
    }

    fn checkpoint(&self) -> ExclusionsCheckpoint {
        ExclusionsCheckpoint(self.placed.len() as u64)
    }

    fn rewind(&mut self, to: ExclusionsCheckpoint) {
        self.placed.truncate(to.0 as usize);
    }
}

impl LayoutPassState<'_> {
    pub(crate) fn compute_inline_layout_winkin(
        &mut self,
        node_id: NodeId,
        mut inline_layout: Box<TextLayout>,
        frame: Frame,
        block_ctx: &mut BlockContext<'_>,
    ) -> LayoutOutput {
        let Frame {
            inputs,
            node_size,
            node_min_size,
            node_max_size,
            aspect_ratio,
            padding,
            border,
            scrollbar_gutter,
            container_pb,
            content_box_inset,
            child_inputs,
            available_space,
            collapses_through,
            scale,
        } = frame;
        let margin = self.nodes[node_id]
            .layout_style()
            .margin()
            .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
        // Where the absolutely positioned boxes are placed from: the
        // padding box, less any scrollbar.
        let oof_position_inset = Rect {
            left: border.left,
            right: border.right + scrollbar_gutter.x,
            top: border.top,
            bottom: border.bottom + scrollbar_gutter.y,
        };
        let mut oof_candidates = OofCandidates::new();
        let known_dimensions = inputs.known_dimensions;
        // The containing block's inline size, which percentages of it are
        // taken of.
        let basis = match available_space.width {
            AvailableSpace::Definite(width) => width,
            _ => 0.0,
        };
        // Every box is laid out, not only measured: winkin reads an atomic
        // inline's baseline with its size.
        let child_inputs = LayoutInput {
            run_mode: RunMode::PerformLayout,
            ..child_inputs
        };

        // Whether the lines run down the page, which turns what taffy
        // measures across them.
        let vertical = {
            let node = &self.nodes[node_id];
            node.primary_styles()
                .map(|computed| computed.writing_mode.is_vertical())
                .or_else(|| {
                    node.parent.and_then(|parent| {
                        self.nodes[parent]
                            .primary_styles()
                            .map(|computed| computed.writing_mode.is_vertical())
                    })
                })
                .unwrap_or(false)
        };

        // The atomic inlines and floats as taffy measures them, which the
        // content is built with: each border box, and where an atomic
        // inline's baseline is. In a vertical line a box's height is along
        // it, and it sits on no baseline of its own.
        // The inline root's layout children are its atomic inlines and
        // floats, in content order.
        let ids: Vec<u64> = self.nodes[node_id]
            .layout_children
            .borrow()
            .iter()
            .flatten()
            .map(|child| child.as_u64())
            .collect();
        let mut sizes = Vec::with_capacity(ids.len());
        #[cfg(feature = "floats")]
        let mut clears = Vec::new();
        for id in ids {
            let node = NodeId::from_u64(id);
            let Some(measured) = self.measure_inline_box(node, &child_inputs, basis) else {
                continue;
            };
            #[cfg(feature = "floats")]
            if self
                .child_layout_style(&self.nodes[node])
                .float()
                .is_floated()
            {
                clears.push((id, self.child_layout_style(&self.nodes[node]).clear()));
            }
            let size = if vertical {
                BoxSize {
                    inline: measured.size.height * scale,
                    block: measured.size.width * scale,
                    baseline: None,
                }
            } else {
                BoxSize {
                    inline: measured.size.width * scale,
                    block: measured.size.height * scale,
                    baseline: measured.baseline.map(|baseline| baseline * scale),
                }
            };
            sizes.push(text_winkin::BoxMeasure { node: id, size });
        }
        if !inline_layout.winkin.is_built_with(&sizes, basis) {
            let doc = &mut *self.doc;
            let guard = doc.guard.read();
            text_winkin::build(
                &doc.nodes,
                text_winkin::Cascade {
                    stylist: &doc.stylist,
                    guards: &StylesheetGuards::same(&guard),
                },
                &mut doc.winkin.cx,
                &mut inline_layout.winkin,
                scale,
                basis,
                node_id,
                &sizes,
            );
        }

        // A vertical block: its lines run down the content box, so the room
        // along them is the box's height, and how far they stack across is
        // its width. Taffy has no writing modes, so the box is sized here in
        // physical terms, and what the lines place is turned onto the page.
        // The floats of the blocks around it are not in its lines' way: taffy
        // places them in horizontal terms.
        if inline_layout.winkin.is_vertical() {
            let writing_mode = inline_layout.winkin.writing_mode();
            let pbh = container_pb.vertical_components().sum() * scale;
            // Its own height, or where it has none the height it has room
            // for, or failing both as long as its longest line. A block in a
            // block container whose lines run the same way stretches into
            // the room; any other box fits its content into it.
            let fit = |along: f32| {
                let widths = inline_layout.winkin.content_widths();
                along.min(widths.max_content).max(widths.min_content).ceil()
            };
            let along = match known_dimensions.height.or(node_size.height) {
                Some(height) => (height * scale) - pbh,
                None => {
                    let room = match available_space.height {
                        AvailableSpace::Definite(height) => Some(height),
                        _ => None,
                    };
                    // The initial containing block's height, which an
                    // orthogonal flow without a definite parent height fits
                    // into.
                    let icb = self.stylist.device().au_viewport_size().height.to_f32_px();
                    // An orthogonal flow whose parent has no definite height
                    // has at most the parent's `max-height`, as Chrome takes
                    // its fallback inline size.
                    let room = match self.parent_max_height(node_id) {
                        Some(cap) => {
                            let cap = (cap.min(icb) - margin.vertical_components().sum()).max(0.0);
                            Some(room.map_or(cap, |room| room.min(cap)))
                        }
                        None => room,
                    };
                    // Asked for its intrinsic height, it answers its
                    // lines' intrinsic length.
                    let intrinsic = inputs.run_mode == RunMode::ComputeSize
                        && inputs.axis == RequestedAxis::Vertical;
                    let widths = inline_layout.winkin.content_widths();
                    match room {
                        None if intrinsic
                            && available_space.height == AvailableSpace::MinContent =>
                        {
                            widths.min_content.ceil()
                        }
                        None if intrinsic => widths.max_content.ceil(),
                        Some(room) if self.stretches_along(node_id, vertical) => {
                            (room * scale) - pbh
                        }
                        Some(room) => fit((room * scale) - pbh),
                        // Within a parent of definite height, an orthogonal
                        // flow fits into that height, less its margins, and
                        // within any other parent into the initial
                        // containing block.
                        None => match inputs.parent_size.height {
                            Some(height) => {
                                fit((height - margin.vertical_components().sum()) * scale - pbh)
                            }
                            None if self.is_orthogonal(node_id, vertical) => {
                                fit((icb - margin.vertical_components().sum()) * scale - pbh)
                            }
                            None => widths.max_content.ceil(),
                        },
                    }
                }
            };
            let mut room = FloatRoom {
                #[cfg(feature = "floats")]
                outer: None,
                #[cfg(not(feature = "floats"))]
                outer: core::marker::PhantomData,
                width: along,
                scale,
                #[cfg(feature = "floats")]
                clears: &clears,
                placed: Vec::new(),
            };
            // The padding at the block's start and end, which a vertical
            // line's over and under sides face: the right and left in the
            // right-to-left modes, the left and right in the others.
            let (start_padding, end_padding) = match writing_mode {
                WritingMode::VerticalRl | WritingMode::SidewaysRl => (padding.right, padding.left),
                _ => (padding.left, padding.right),
            };
            let area = Area {
                room_above: start_padding * scale,
                ..Area::new(along)
            };
            let laid = inline_layout
                .winkin
                .lay_out(&mut self.winkin.cx, area, &mut room);
            let floats = std::mem::take(&mut room.placed);
            drop(room);
            // What the lines placed, along them and across the block from
            // its start, in device pixels: each atomic inline's margin box,
            // and where each absolutely positioned box would have been.
            let mut placed: Vec<(NodeId, [f32; 4], Option<bool>)> = Vec::new();
            let across = match laid {
                Some(layout) => {
                    for line in layout.lines() {
                        let metrics = line.metrics();
                        // `vertical-lr` stacks its lines from the left while
                        // each line's over side is its right.
                        let block = |over: f32, under: f32| {
                            if writing_mode == WritingMode::VerticalLr {
                                let bottom = metrics.top + metrics.height();
                                [bottom - under, bottom - over]
                            } else {
                                [metrics.top + over, metrics.top + under]
                            }
                        };
                        for item in line.items() {
                            if let Item::Atomic(atomic) = item {
                                let inline = atomic.inline();
                                let cross = atomic.block();
                                let [start, end] = block(cross.over, cross.under);
                                placed.push((
                                    NodeId::from_u64(atomic.key().0),
                                    [
                                        metrics.left + inline.left,
                                        metrics.left + inline.right,
                                        start,
                                        end,
                                    ],
                                    None,
                                ));
                            }
                        }
                    }
                    // Where each absolutely positioned box would have been.
                    // Its block-start edge, across the page, faces right
                    // where the lines stack from the right.
                    let from_right = matches!(
                        writing_mode,
                        WritingMode::VerticalRl | WritingMode::SidewaysRl
                    );
                    for at in layout.static_positions() {
                        placed.push((
                            NodeId::from_u64(at.key.0),
                            [at.inline, at.inline, at.block, at.block],
                            Some(from_right),
                        ));
                    }
                    let metrics = layout.metrics();
                    metrics.block_end - into_end_padding(layout.room_below(), end_padding * scale)
                }
                None => 0.0,
            };
            // The floats reach across the block too.
            let across = floats
                .iter()
                .map(|float| float.bottom)
                .fold(across, f32::max);
            let measured_size = Size {
                width: across.ceil() / scale,
                height: along / scale,
            };
            // A vertical block in a horizontal one is an orthogonal flow:
            // where its width is `auto` it is as wide as its lines stack, as
            // Chrome sizes it, rather than stretched across its container.
            let auto_width = self.nodes[node_id].layout_style().size().width.is_auto();
            let own_size = Size {
                width: if auto_width {
                    None
                } else {
                    known_dimensions.width.or(node_size.width)
                },
                height: known_dimensions.height.or(node_size.height),
            };
            let final_size = own_size
                .unwrap_or(measured_size + content_box_inset.sum_axes())
                .maybe_clamp(node_min_size, node_max_size)
                .maybe_max(container_pb.sum_axes().map(Some));
            // Onto the content box: the top left of what reaches `along` the
            // lines and `across` the block, in device pixels.
            let content = (final_size - content_box_inset.sum_axes()).map(|size| size * scale);
            let page = |[left, right, start, end]: [f32; 4]| match writing_mode {
                WritingMode::VerticalRl | WritingMode::SidewaysRl => (content.width - end, left),
                WritingMode::VerticalLr => (start, left),
                WritingMode::SidewaysLr => (start, content.height - right),
                WritingMode::HorizontalTb => (left, start),
            };
            let container_direction = self.nodes[node_id].layout_style().direction();
            for (order, (node, extents, rtl)) in placed.into_iter().enumerate() {
                let (x, y) = page(extents);
                self.place_inline_box(
                    &Placed {
                        node,
                        x,
                        top: y,
                        line_top: y,
                        line_bottom: y,
                        rtl,
                    },
                    order as u32,
                    &child_inputs,
                    container_pb,
                    content_box_inset,
                    final_size,
                    container_direction,
                    scale,
                    &mut oof_candidates,
                );
            }
            #[cfg(feature = "floats")]
            for float in &floats {
                if float.key & (text_winkin::FIRST_LETTER_KEY | text_winkin::MARKER_KEY) != 0 {
                    continue;
                }
                let (x, y) = page([float.left, float.right, float.top, float.bottom]);
                let turned = Floated {
                    left: x,
                    right: x,
                    top: y,
                    bottom: y,
                    ..*float
                };
                self.place_float(
                    NodeId::from_u64(float.key),
                    &turned,
                    &child_inputs,
                    basis,
                    container_pb,
                    scale,
                );
            }
            #[cfg(not(feature = "floats"))]
            let _ = floats;
            inline_layout.block_offset = 0.0;
            self.put_inline_layout(node_id, inline_layout);
            let content_extent = measured_size + padding.sum_axes();
            return LayoutOutput {
                size: final_size,
                scrollable_overflow_rect: Rect {
                    left: 0.0,
                    right: content_extent.width,
                    top: 0.0,
                    bottom: content_extent.height,
                },
                baselines: taffy::Baselines {
                    first: None,
                    last: None,
                },
                top_margin: CollapsibleMarginSet::ZERO,
                bottom_margin: CollapsibleMarginSet::ZERO,
                margins_can_collapse_through: false,
                oof_candidates,
                oof_positioning_area: Some(OofPositioningArea {
                    size: final_size - oof_position_inset.sum_axes(),
                    offset: Point {
                        x: oof_position_inset.left,
                        y: oof_position_inset.top,
                    },
                }),
            };
        }

        let pbw = container_pb.horizontal_components().sum() * scale;
        let width = known_dimensions
            .width
            .map(|width| (width * scale) - pbw)
            .unwrap_or_else(|| {
                let widths = inline_layout.winkin.content_widths();
                let computed = match available_space.width {
                    AvailableSpace::MinContent => widths.min_content,
                    AvailableSpace::MaxContent => widths.max_content,
                    AvailableSpace::Definite(limit) => (limit * scale)
                        .min(widths.max_content)
                        .max(widths.min_content),
                }
                .ceil();
                let style_width = node_size.width.map(|width| width * scale);
                let min_width = node_min_size.width.map(|width| width * scale);
                let max_width = node_max_size.width.map(|width| width * scale);
                style_width
                    .unwrap_or(computed + pbw)
                    .max(computed)
                    .maybe_clamp(min_width, max_width)
                    - pbw
            });

        #[cfg(feature = "floats")]
        let is_bfc_root = block_ctx.is_bfc_root();
        #[cfg(feature = "floats")]
        if is_bfc_root {
            block_ctx.set_width((width + pbw) / scale);
        }
        // The floats are placed against the content box, inside the padding
        // and border.
        #[cfg(feature = "floats")]
        let mut block_ctx =
            block_ctx.sub_context(container_pb.top, [container_pb.left, container_pb.right]);
        #[cfg(not(feature = "floats"))]
        let _ = block_ctx;

        if inputs.run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::Horizontal {
            self.put_inline_layout(node_id, inline_layout);
            let measured_size = known_dimensions.unwrap_or(Size {
                width: width.ceil() / scale,
                height: 0.0,
            });
            let clamped_size = known_dimensions
                .or(node_size)
                .unwrap_or(measured_size + content_box_inset.sum_axes())
                .maybe_clamp(node_min_size, node_max_size)
                .maybe_max(container_pb.sum_axes().map(Some));
            return LayoutOutput::from_outer_size(clamped_size);
        }

        let mut room = FloatRoom {
            #[cfg(feature = "floats")]
            outer: Some(&block_ctx),
            #[cfg(not(feature = "floats"))]
            outer: core::marker::PhantomData,
            width,
            scale,
            #[cfg(feature = "floats")]
            clears: &clears,
            placed: Vec::new(),
        };
        // Where the content box ends where its `height` or `max-height`
        // puts an end to it, which `line-clamp: auto` keeps the lines within.
        let inset_height = content_box_inset.vertical_components().sum();
        let block_end = known_dimensions
            .height
            .or(node_size.height)
            .or(node_max_size.height)
            .map(|height| (height - inset_height).max(0.0) * scale);
        let area = Area {
            room_above: self.room_above(node_id, margin.top, padding.top, container_pb.top, inputs)
                * scale,
            block_end,
            ..Area::new(width)
        };

        let mut placed = Vec::new();
        let laid = inline_layout
            .winkin
            .lay_out(&mut self.winkin.cx, area, &mut room);
        let floats = std::mem::take(&mut room.placed);
        drop(room);
        let (height, first_baseline, last_baseline) = match laid {
            Some(layout) => {
                for line in layout.lines() {
                    let metrics = line.metrics();
                    for item in line.items() {
                        if let Item::Atomic(atomic) = item {
                            let inline = atomic.inline();
                            let block = atomic.block();
                            placed.push(Placed {
                                node: NodeId::from_u64(atomic.key().0),
                                x: metrics.left + inline.left,
                                top: metrics.top + block.over,
                                line_top: metrics.top,
                                line_bottom: metrics.top + metrics.height(),
                                rtl: None,
                            });
                        }
                    }
                }
                // Where each absolutely positioned box would have been, as
                // winkin finds its static position: its line box spans the
                // block axis of an inline-level box's static-position
                // rectangle.
                for at in layout.static_positions() {
                    let bottom =
                        at.line
                            .and_then(|line| layout.line(line))
                            .map_or(at.block, |line| {
                                let metrics = line.metrics();
                                (metrics.top + metrics.height()).max(at.block)
                            });
                    placed.push(Placed {
                        node: NodeId::from_u64(at.key.0),
                        x: at.inline,
                        top: at.block,
                        line_top: at.block,
                        line_bottom: bottom,
                        rtl: Some(at.direction == TextDirection::Rtl),
                    });
                }
                let metrics = layout.metrics();
                (
                    metrics.block_end
                        - into_end_padding(layout.room_below(), padding.bottom * scale),
                    metrics.first_baseline,
                    metrics.last_baseline,
                )
            }
            None => (0.0, None, None),
        };
        // A block formatting context's root holds its floats.
        #[cfg(feature = "floats")]
        let height = if is_bfc_root {
            floats
                .iter()
                .map(|float| float.bottom)
                .fold(height, f32::max)
        } else {
            height
        };

        let measured_size = Size {
            width: width / scale,
            height: height / scale,
        };
        let clamped_size = known_dimensions
            .or(node_size)
            .unwrap_or(measured_size + content_box_inset.sum_axes())
            .maybe_clamp(node_min_size, node_max_size);
        let final_size = Size {
            width: clamped_size.width,
            height: f32_max(
                clamped_size.height,
                aspect_ratio
                    .map(|ratio| clamped_size.width / ratio)
                    .unwrap_or(0.0),
            ),
        }
        .maybe_max(container_pb.sum_axes().map(Some));

        // `align-content` moves the lines, as one, within the content box;
        // the floats stay where the block formatting context put them.
        let align_content =
            taffy::BlockContainerStyle::align_content(&self.nodes[node_id].layout_style());
        let block_offset = if align_content.keyword() == taffy::AlignContentKeyword::Normal {
            0.0
        } else {
            let free_space =
                final_size.height - content_box_inset.vertical_axis_sum() - measured_size.height;
            taffy::compute_block_align_content_offset(align_content, free_space)
        };
        inline_layout.block_offset = block_offset;
        let lines_pb = Rect {
            top: container_pb.top + block_offset,
            ..container_pb
        };

        let container_direction = self.nodes[node_id].layout_style().direction();
        for (order, at) in placed.into_iter().enumerate() {
            self.place_inline_box(
                &at,
                order as u32,
                &child_inputs,
                lines_pb,
                content_box_inset,
                final_size,
                container_direction,
                scale,
                &mut oof_candidates,
            );
        }
        // Each float goes where the lines flowed around it, and into the
        // block formatting context for the blocks after this one. The block's
        // initial letter goes into the context alone: its box is painted
        // from the lines.
        #[cfg(feature = "floats")]
        if inputs.run_mode == RunMode::PerformLayout {
            for float in &floats {
                let direction = match float.side {
                    FloatSide::Left => taffy::FloatDirection::Left,
                    FloatSide::Right => taffy::FloatDirection::Right,
                };
                let clear = clears
                    .iter()
                    .find(|(node, _)| *node == float.key)
                    .map_or(taffy::Clear::None, |(_, clear)| *clear);
                block_ctx.place_floated_box(
                    Size {
                        width: (float.right - float.left) / scale,
                        height: (float.bottom - float.top) / scale,
                    },
                    float.top / scale,
                    direction,
                    clear,
                    false,
                );
                if float.key & (text_winkin::FIRST_LETTER_KEY | text_winkin::MARKER_KEY) == 0 {
                    self.place_float(
                        NodeId::from_u64(float.key),
                        float,
                        &child_inputs,
                        basis,
                        container_pb,
                        scale,
                    );
                }
            }
        }
        #[cfg(not(feature = "floats"))]
        let _ = floats;

        self.put_inline_layout(node_id, inline_layout);

        LayoutOutput {
            size: final_size,
            scrollable_overflow_rect: {
                let content_extent = measured_size + padding.sum_axes();
                Rect {
                    left: 0.0,
                    right: content_extent.width,
                    top: 0.0,
                    bottom: content_extent.height + block_offset.max(0.0),
                }
            },
            baselines: taffy::Baselines {
                first: first_baseline.map(|baseline| (baseline / scale) + lines_pb.top),
                last: last_baseline.map(|baseline| (baseline / scale) + lines_pb.top),
            },
            top_margin: CollapsibleMarginSet::ZERO,
            bottom_margin: CollapsibleMarginSet::ZERO,
            margins_can_collapse_through: !collapses_through
                && final_size.height == 0.0
                && measured_size.height == 0.0,
            oof_candidates,
            oof_positioning_area: Some(OofPositioningArea {
                size: final_size - oof_position_inset.sum_axes(),
                offset: Point {
                    x: oof_position_inset.left,
                    y: oof_position_inset.top,
                },
            }),
        }
    }

    /// Whether vertical block `node_id` stretches along its lines into the
    /// room it is given: whether it is an in-flow block in a block
    /// container whose lines run the same way.
    fn stretches_along(&self, node_id: NodeId, vertical: bool) -> bool {
        let node = &self.nodes[node_id];
        if let Some(style) = node.primary_styles() {
            let display = style.clone_display();
            if display.outside() != DisplayOutside::Block
                || style.get_box().float != style::computed_values::float::T::None
                || style.get_box().position.is_absolutely_positioned()
            {
                return false;
            }
        }
        let Some(parent) = node.layout_parent.get().map(|id| &self.nodes[id]) else {
            return false;
        };
        parent.taffy_display() == taffy::Display::Block
            && parent
                .primary_styles()
                .is_some_and(|style| style.writing_mode.is_vertical() == vertical)
    }

    /// Whether vertical block `node_id` is an orthogonal flow: whether its
    /// layout parent's lines run across its own.
    fn is_orthogonal(&self, node_id: NodeId, vertical: bool) -> bool {
        self.nodes[node_id]
            .layout_parent
            .get()
            .and_then(|parent| self.nodes[parent].primary_styles())
            .is_some_and(|style| style.writing_mode.is_vertical() != vertical)
    }

    /// The content height `node_id`'s layout parent may grow to where its
    /// own height is `auto` and its `max-height` a length: that length, no
    /// less than its `min-height`, in CSS pixels.
    fn parent_max_height(&self, node_id: NodeId) -> Option<f32> {
        let parent = self.nodes[node_id].layout_parent.get()?;
        let style = self.child_layout_style(&self.nodes[parent]);
        if !style.size().height.is_auto() {
            return None;
        }
        // The parent's own percentages are of a box this does not know.
        let unknown: Option<f32> = None;
        let max = style
            .max_size()
            .height
            .maybe_resolve(unknown, resolve_calc_value)?;
        let min = style
            .min_size()
            .height
            .maybe_resolve(unknown, resolve_calc_value)
            .unwrap_or(0.0);
        let edges = if style.box_sizing() == taffy::BoxSizing::BorderBox {
            let padding = style.padding().resolve_or_zero(unknown, resolve_calc_value);
            let border = style.border().resolve_or_zero(unknown, resolve_calc_value);
            (padding + border).vertical_components().sum()
        } else {
            0.0
        };
        Some((max.max(min) - edges).max(0.0))
    }

    /// The room over the block's first line its ruby annotations and
    /// emphasis marks may take before the line moves down for them, in CSS
    /// pixels, as Chrome's block layout lends it
    /// (`ComputeInitialBlockStartAnnotationSpace`): the block's padding over
    /// the line, and where no border is in the way, the margin collapsed
    /// between it and the block before it, that block's end padding, and the
    /// room that block's last line left under its content.
    fn room_above(
        &self,
        node_id: NodeId,
        margin_top: f32,
        padding_top: f32,
        padding_border_top: f32,
        inputs: LayoutInput,
    ) -> f32 {
        let mut room = padding_top;
        if padding_border_top > padding_top {
            return room;
        }
        let own = margin_top.max(0.0);
        let Some(before) = self.block_before(node_id) else {
            return room + own;
        };
        let node = &self.nodes[before];
        let style = self.child_layout_style(node);
        let parent_size = inputs.parent_size;
        let margin = style
            .margin()
            .resolve_or_zero(parent_size, resolve_calc_value)
            .bottom
            .max(0.0);
        let padding = style
            .padding()
            .resolve_or_zero(parent_size, resolve_calc_value)
            .bottom;
        let border = style
            .border()
            .resolve_or_zero(parent_size, resolve_calc_value)
            .bottom;
        drop(style);
        room += own.max(margin);
        if border > 0.0 {
            return room;
        }
        let lent = node
            .element_data()
            .and_then(|element| element.inline_layout_data.as_ref())
            .and_then(|text| text.winkin.layout())
            .map_or(0.0, |layout| layout.room_below() / self.viewport.scale());
        room + padding + lent.max(0.0)
    }

    /// The in-flow block laid out before `node_id` among its layout
    /// siblings, if there is one.
    fn block_before(&self, node_id: NodeId) -> Option<NodeId> {
        let parent = self.nodes[node_id].layout_parent.get()?;
        let children = self.nodes[parent].layout_children.borrow();
        let children = children.as_ref()?;
        let at = children.iter().position(|child| *child == node_id)?;
        children[..at].iter().rev().copied().find(|&child| {
            let node = &self.nodes[child];
            if !matches!(
                node.data,
                crate::NodeData::Element(_) | crate::NodeData::AnonymousBlock(_)
            ) {
                return false;
            }
            node.primary_styles().is_some_and(|computed| {
                !computed.get_box().display.is_none()
                    && !computed.clone_position().is_absolutely_positioned()
                    && matches!(computed.clone_float(), style::values::computed::Float::None)
            })
        })
    }

    fn put_inline_layout(&mut self, node_id: NodeId, inline_layout: Box<TextLayout>) {
        self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .inline_layout_data = Some(inline_layout);
    }

    /// Lays out an atomic inline or a float as the line will hold it: its
    /// border box, in CSS pixels, and an atomic inline's baseline down from
    /// its top. `None` for an absolutely positioned box, which the content
    /// holds only a place for.
    ///
    /// A float is sized in the room beside nothing, less its margins, which
    /// taffy shrinks it to fit. An inline-block's baseline is its last line
    /// box's, and a flex, grid or table box's its first; a replaced element,
    /// or an inline-block that clips its overflow or has no line box, has
    /// none, and sits on its margin box's bottom.
    fn measure_inline_box(
        &mut self,
        node: NodeId,
        child_inputs: &LayoutInput,
        basis: f32,
    ) -> Option<Measured> {
        let (floated, uses_last, has_baseline, margin, width_style) = {
            let held = &self.nodes[node];
            let style = self.child_layout_style(held);
            if style.position().is_out_of_flow() {
                return None;
            }
            #[cfg(feature = "floats")]
            let floated = style.float().is_floated();
            #[cfg(not(feature = "floats"))]
            let floated = false;
            let margin = style
                .margin()
                .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);
            let replaced = held
                .data
                .downcast_element()
                .is_some_and(|element| is_replaced_element(&element.name.local));
            let display = held.display_style();
            let inside = display.map(|display| display.inside());
            // `baseline-source: auto` is the last baseline for an
            // inline-block and the first otherwise, and only then does an
            // inline-block that clips sit on its margin box's bottom.
            let source = held
                .primary_styles()
                .map_or(BaselineSource::Auto, |computed| {
                    computed.clone_baseline_source()
                });
            let uses_last = match source {
                BaselineSource::First => false,
                BaselineSource::Last => true,
                BaselineSource::Auto => {
                    matches!(inside, Some(DisplayInside::Flow | DisplayInside::FlowRoot))
                }
            };
            let clips = source == BaselineSource::Auto
                && held.primary_styles().is_some_and(|computed| {
                    !matches!(
                        computed.clone_overflow_x(),
                        style::values::computed::Overflow::Visible
                    ) || !matches!(
                        computed.clone_overflow_y(),
                        style::values::computed::Overflow::Visible
                    )
                });
            (
                floated,
                uses_last,
                !replaced && !(uses_last && clips),
                margin,
                style.size().width,
            )
        };
        let inputs = if floated {
            LayoutInput {
                known_dimensions: Size::NONE,
                sizing_mode: SizingMode::InherentSize,
                available_space: Size {
                    width: match child_inputs.available_space.width {
                        AvailableSpace::Definite(_) => {
                            AvailableSpace::Definite((basis - margin.left - margin.right).max(0.0))
                        }
                        other => other,
                    },
                    height: AvailableSpace::MaxContent,
                },
                ..*child_inputs
            }
        } else {
            inline_box_inputs(width_style, margin, *child_inputs)
        };
        let output = self.compute_child_layout(taffy::NodeId::from(node.as_u64()), inputs);
        let baseline = if has_baseline && !floated {
            if uses_last {
                // An inline formatting context's own baseline is its layout's;
                // a block container's is found among its children.
                let inline_root = self.nodes[node]
                    .element_data()
                    .is_some_and(|element| element.inline_layout_data.is_some());
                let walked = if inline_root {
                    LastBaseline::Unknown
                } else {
                    self.last_line_baseline(node)
                };
                match walked {
                    LastBaseline::At(baseline) => Some(baseline),
                    LastBaseline::None => None,
                    LastBaseline::Unknown => output.baselines.last.or(output.baselines.first),
                }
            } else {
                output.baselines.first.or(output.baselines.last)
            }
        } else {
            None
        };
        Some(Measured {
            size: output.size,
            baseline,
        })
    }

    /// An inline-block's baseline, its last line box's, down from the border
    /// box's top of `node`, a block container laid out already, in CSS
    /// pixels. The last in-flow child in its writing mode that has one
    /// gives it, and a child that is a scroll container gives its margin
    /// box's bottom edge, as Chrome takes an inline-block's baseline.
    fn last_line_baseline(&self, node: NodeId) -> LastBaseline {
        let held = &self.nodes[node];
        if let Some(text) = held
            .element_data()
            .and_then(|element| element.inline_layout_data.as_ref())
        {
            let Some(layout) = text.winkin.layout() else {
                return LastBaseline::Unknown;
            };
            if text.winkin.is_vertical() {
                return LastBaseline::Unknown;
            }
            let Some(baseline) = layout.metrics().last_baseline else {
                return LastBaseline::None;
            };
            let edges = held.unrounded_layout();
            return LastBaseline::At(
                (baseline / self.viewport.scale()) + edges.border.top + edges.padding.top,
            );
        }
        let vertical = held
            .primary_styles()
            .is_some_and(|computed| computed.writing_mode.is_vertical());
        let children = held.layout_children.borrow();
        let Some(children) = children.as_ref() else {
            return LastBaseline::None;
        };
        for &child in children.iter().rev() {
            let child_node = &self.nodes[child];
            let Some(computed) = child_node.primary_styles() else {
                continue;
            };
            // An orthogonal flow gives no baseline across this one's lines.
            if computed.get_box().display.is_none()
                || computed.writing_mode.is_vertical() != vertical
                || computed.clone_position().is_absolutely_positioned()
                || !matches!(computed.clone_float(), style::values::computed::Float::None)
            {
                continue;
            }
            let scrolls = !matches!(
                computed.clone_overflow_x(),
                style::values::computed::Overflow::Visible
            ) || !matches!(
                computed.clone_overflow_y(),
                style::values::computed::Overflow::Visible
            );
            let inside = child_node.display_style().map(|display| display.inside());
            let flows = matches!(inside, Some(DisplayInside::Flow | DisplayInside::FlowRoot));
            drop(computed);
            let placed = child_node.unrounded_layout();
            if scrolls && flows {
                return LastBaseline::At(
                    placed.location.y + placed.size.height + placed.margin.bottom,
                );
            }
            if !flows {
                return LastBaseline::Unknown;
            }
            match self.last_line_baseline(child) {
                LastBaseline::At(baseline) => {
                    return LastBaseline::At(placed.location.y + baseline);
                }
                LastBaseline::None => continue,
                LastBaseline::Unknown => return LastBaseline::Unknown,
            }
        }
        LastBaseline::None
    }

    /// Sets where a float the lines flowed around goes.
    #[cfg(feature = "floats")]
    fn place_float(
        &mut self,
        node: NodeId,
        float: &Floated,
        child_inputs: &LayoutInput,
        basis: f32,
        container_pb: Rect<f32>,
        scale: f32,
    ) {
        let Some(measured) = self.measure_inline_box(node, child_inputs, basis) else {
            return;
        };
        let (margin, padding, border) = {
            let style = self.child_layout_style(&self.nodes[node]);
            (
                style
                    .margin()
                    .resolve_or_zero(child_inputs.parent_size, resolve_calc_value),
                style
                    .padding()
                    .resolve_or_zero(child_inputs.parent_size, resolve_calc_value),
                style
                    .border()
                    .resolve_or_zero(child_inputs.parent_size, resolve_calc_value),
            )
        };
        let layout = self.nodes[node].unrounded_layout_mut();
        layout.size = measured.size;
        layout.location.x = (float.left / scale) + margin.left + container_pb.left;
        layout.location.y = (float.top / scale) + margin.top + container_pb.top;
        layout.padding = padding;
        layout.border = border;
        layout.margin = margin;
    }

    /// Sets where an atomic inline, or an absolutely positioned box's static
    /// position, goes, as the Parley path does for its inline boxes.
    ///
    /// An absolutely positioned box is handed to its containing block as a
    /// candidate placed from its static position, as is every one the box
    /// holds.
    #[allow(clippy::too_many_arguments)]
    fn place_inline_box(
        &mut self,
        at: &Placed,
        order: u32,
        child_inputs: &LayoutInput,
        container_pb: Rect<f32>,
        content_box_inset: Rect<f32>,
        final_size: Size<f32>,
        container_direction: Direction,
        scale: f32,
        oof_candidates: &mut OofCandidates,
    ) {
        let node = &self.nodes[at.node];
        let style = self.child_layout_style(node);
        let padding = style
            .padding()
            .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);
        let border = style
            .border()
            .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);
        let margin = style
            .margin()
            .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);
        let position = style.position();
        let is_absolute = position.is_out_of_flow();
        let item_direction = style.direction();
        // An inline formatting context has no `justify-items` or
        // `align-items` for an `auto` self-alignment to defer to.
        let justify_self =
            taffy::OofItemStyle::justify_self(&style).unwrap_or(taffy::AlignItems::NORMAL);
        let align_self =
            taffy::OofItemStyle::align_self(&style).unwrap_or(taffy::AlignItems::NORMAL);
        let box_inputs = inline_box_inputs(style.size().width, margin, *child_inputs);
        let is_inline_level =
            style.style.get_box().original_display.outside() == DisplayOutside::Inline;
        let container_content_size = final_size - content_box_inset.sum_axes();
        let inset_style = style.inset();
        let inset = Rect {
            left: inset_style
                .left
                .maybe_resolve(container_content_size.width, resolve_calc_value),
            right: inset_style
                .right
                .maybe_resolve(container_content_size.width, resolve_calc_value),
            top: inset_style
                .top
                .maybe_resolve(container_content_size.height, resolve_calc_value),
            bottom: inset_style
                .bottom
                .maybe_resolve(container_content_size.height, resolve_calc_value),
        };
        drop(style);

        if is_absolute {
            // The static-position rectangle, from winkin's static position.
            // An inline-level box's is zero-wide where it stands in the line
            // and spans the line box from its top. A block-level box's spans
            // the content box at the line's top, or under the line where
            // in-flow content comes before it there.
            let line_top = (at.line_top / scale) + container_pb.top;
            let line_bottom = (at.line_bottom / scale) + container_pb.top;
            let (inline_area, block_area) = if is_inline_level {
                let x = (at.x / scale) + container_pb.left;
                (
                    taffy::Line { start: x, end: x },
                    taffy::Line {
                        start: line_top,
                        end: line_bottom,
                    },
                )
            } else {
                (
                    taffy::Line {
                        start: container_pb.left,
                        end: final_size.width - container_pb.right,
                    },
                    taffy::Line {
                        start: line_top,
                        end: line_top,
                    },
                )
            };
            // An inline-level box's start faces the way its anchor's bidi
            // level reads, as in Chrome. In a vertical block the horizontal
            // axis runs across the lines, and its block start faces right
            // where they stack from the right.
            let inline_rtl = match at.rtl {
                Some(rtl) if is_inline_level => rtl,
                _ => container_direction.is_rtl(),
            };
            oof_candidates.push(OofCandidate {
                node: taffy::NodeId::from(at.node.as_u64()),
                order,
                position,
                static_position: Point {
                    x: AxisStaticPosition::from_alignment(
                        justify_self.resolve_self_relative(
                            item_direction,
                            container_direction,
                            true,
                        ),
                        inline_area,
                        inline_rtl,
                    ),
                    y: AxisStaticPosition::from_alignment(
                        align_self.resolve_self_relative(
                            item_direction,
                            container_direction,
                            false,
                        ),
                        block_area,
                        false,
                    ),
                },
            });
            return;
        }

        let mut output =
            self.compute_child_layout(taffy::NodeId::from(at.node.as_u64()), box_inputs);
        let size = output.size;
        let inset_offset = taffy::Point {
            x: if container_direction == Direction::Rtl {
                inset.right.map(|x| -x).or(inset.left).unwrap_or(0.0)
            } else {
                inset.left.or(inset.right.map(|x| -x)).unwrap_or(0.0)
            },
            y: inset.top.or(inset.bottom.map(|y| -y)).unwrap_or(0.0),
        };
        // The inline boxes it is in move it as `position: relative` moves
        // them.
        let shift = self.nodes[at.node]
            .parent
            .map_or(kurbo::Vec2::ZERO, |parent| {
                text_winkin::relative_shift(
                    self,
                    parent.as_u64(),
                    kurbo::Size::new(
                        f64::from(container_content_size.width),
                        f64::from(container_content_size.height),
                    ),
                    container_direction == Direction::Rtl,
                )
            });
        // The layout placed the margin box; the node is its border box.
        let layout = self.nodes[at.node].unrounded_layout_mut();
        layout.size = size;
        layout.location.x =
            (at.x / scale) + margin.left + container_pb.left + inset_offset.x + shift.x as f32;
        layout.location.y =
            (at.top / scale) + margin.top + container_pb.top + inset_offset.y + shift.y as f32;
        layout.padding = padding;
        layout.border = border;
        layout.margin = margin;
        layout.scrollable_overflow_rect = output.scrollable_overflow_rect;
        // The candidates the box holds, from its own corner to this block's.
        if !output.oof_candidates.is_empty() {
            output.oof_candidates.translate(layout.location);
            oof_candidates.append(&mut output.oof_candidates);
        }
    }
}

/// How far the last line's ruby annotations and emphasis marks reach into
/// the block's end padding, `padding` device pixels, where they reach past
/// its line box: the layout's end counts them, and Chrome lets them into
/// the padding rather than make the block taller.
fn into_end_padding(room_below: f32, padding: f32) -> f32 {
    (-room_below).clamp(0.0, padding.max(0.0))
}

/// Where a block container's last line box puts its baseline.
enum LastBaseline {
    /// Down from its border box's top, in CSS pixels.
    At(f32),
    /// It has no line box.
    None,
    /// Something in it this walk does not read, which taffy answers for.
    Unknown,
}

/// An atomic inline or a float as taffy laid it out.
struct Measured {
    /// Its border box, in CSS pixels.
    size: Size<f32>,
    /// Its baseline down from its border box's top, in CSS pixels.
    baseline: Option<f32>,
}
