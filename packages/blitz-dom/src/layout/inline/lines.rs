//! Measuring an inline formatting context's boxes, breaking its lines and placing what they hold,
//! for every text backend.

use blitz_traits::node_id::NodeId;
use style::values::computed::Float as StyloFloat;
use style::values::specified::box_::{BaselineSource, DisplayInside, DisplayOutside};
use taffy::{
    AvailableSpace, AxisStaticPosition, BlockContainerStyle, BlockContext, CollapsibleMarginSet,
    CoreStyle as _, Direction, LayoutOutput, LayoutPartialTree as _, MaybeMath as _,
    MaybeResolve as _, OofCandidate, OofCandidates, OofItemStyle, OofPositioningArea, Overflow,
    Point, RequestedAxis, ResolveOrZero as _, RunMode, Size,
};

#[cfg(feature = "floats")]
use taffy::{BlockItemStyle as _, Clear, Float};

use super::floats::FloatRoom;
#[cfg(feature = "floats")]
use super::floats::Floated;
use super::{Frame, f32_max, float_box_inputs, inline_box_inputs};
use crate::layout::LayoutPassState;
use crate::layout::replaced::is_replaced_element;
use crate::layout::resolve_calc_value;
use crate::node::TextLayout;
use crate::text::{
    BoxMeasure, FloatSide, InlineLayoutEngine as _, InlineText as _, LastBaseline, LineArea,
    LinesInputs, Measure, Placement,
};

impl LayoutPassState<'_> {
    /// Measures the inline formatting context's boxes, breaks its lines and places them and what
    /// they hold, in the room `frame` describes.
    pub(super) fn lay_out_lines(
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
        let known_dimensions = inputs.known_dimensions;

        // Short circuit if inline context contains no text or inline boxes
        if !collapses_through && inline_layout.is_empty() {
            self.put_inline_layout(node_id, inline_layout);
            return LayoutOutput::from_outer_size(
                Size::ZERO.maybe_max(container_pb.sum_axes().map(Some)),
            );
        }

        // Whether the lines run down the page, where the backend sets them so, which turns what
        // Taffy measures across them.
        let vertical = TextLayout::SETS_WRITING_MODES && {
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
        // What the pass needs of the atomic inlines, which says how far each is laid out to
        // measure it. A pass that places them lays them out.
        let pass = match inputs.run_mode {
            RunMode::ComputeSize if !vertical && inputs.axis == RequestedAxis::Horizontal => {
                Measure::InlineSizes
            }
            RunMode::ComputeSize => Measure::Sizes,
            _ => Measure::Layout,
        };
        // Where the boxes are placed, they are laid out. The requested axis means nothing to a
        // full layout, so every pass asks for one alike.
        let child_inputs = taffy::LayoutInput {
            run_mode: RunMode::PerformLayout,
            axis: RequestedAxis::Both,
            ..child_inputs
        };
        // The containing block's inline size, which a float's room is taken of.
        let basis = match available_space.width {
            AvailableSpace::Definite(width) => width,
            _ => 0.0,
        };
        let sizes = self.measure_inline_boxes(node_id, child_inputs, pass, basis, vertical);
        inline_layout.prepare(
            self,
            node_id,
            LinesInputs {
                sizes: &sizes,
                pass,
                basis,
                vertical,
            },
        );
        // Each float's `clear`, which the floats it reaches below are placed under.
        #[cfg(feature = "floats")]
        let clears: Vec<(u64, Clear)> = sizes
            .iter()
            .filter(|measured| measured.float.is_some())
            .map(|measured| {
                let style = self.child_layout_style(&self.nodes[measured.node]);
                (measured.node.as_u64(), style.clear())
            })
            .collect();

        if inline_layout.line_flow().is_vertical() {
            return self.lay_out_vertical_lines(
                node_id,
                inline_layout,
                frame,
                LinePass {
                    pass,
                    child_inputs,
                    basis,
                    #[cfg(feature = "floats")]
                    clears: &clears,
                    #[cfg(not(feature = "floats"))]
                    clears: core::marker::PhantomData,
                },
            );
        }

        let pbw = container_pb.horizontal_components().sum() * scale;
        let width = known_dimensions
            .width
            .map(|w| (w * scale) - pbw)
            .unwrap_or_else(|| {
                let widths = inline_layout.content_widths();
                let computed_width = match available_space.width {
                    AvailableSpace::MinContent => widths.min,
                    AvailableSpace::MaxContent => widths.max,
                    AvailableSpace::Definite(limit) => {
                        (limit * scale).min(widths.max).max(widths.min)
                    }
                }
                .ceil();

                let style_width = node_size.width.map(|w| w * scale);
                let min_width = node_min_size.width.map(|w| w * scale);
                let max_width = node_max_size.width.map(|w| w * scale);

                (style_width)
                    .unwrap_or(computed_width + pbw)
                    .max(computed_width)
                    .maybe_clamp(min_width, max_width)
                    - pbw
            });

        // Set block context width if this is a block context root
        #[cfg(feature = "floats")]
        let is_bfc_root = block_ctx.is_bfc_root();
        #[cfg(feature = "floats")]
        if is_bfc_root {
            block_ctx.set_width((width + pbw) / scale);
        }

        // Create sub-context to account for the inline layout's padding/border
        #[cfg(feature = "floats")]
        let mut block_ctx =
            block_ctx.sub_context(container_pb.top, [container_pb.left, container_pb.right]);
        #[cfg(not(feature = "floats"))]
        let _ = block_ctx;

        if inputs.run_mode == taffy::RunMode::ComputeSize
            && inputs.axis == RequestedAxis::Horizontal
        {
            self.put_inline_layout(node_id, inline_layout);

            let measured_size = inputs.known_dimensions.unwrap_or(taffy::Size {
                width: width.ceil() / scale,
                // Height is ignored if RequestedAxis if Horizontal
                height: 0.0,
            });

            let clamped_size = inputs
                .known_dimensions
                .or(node_size)
                .unwrap_or(measured_size + content_box_inset.sum_axes())
                .maybe_clamp(node_min_size, node_max_size)
                .maybe_max(container_pb.sum_axes().map(Some));

            return LayoutOutput::from_outer_size(clamped_size);
        }

        // Out-of-flow candidates bubbled up from this container and its in-flow subtree.
        // These are laid out by the out-of-flow positioning pass (`compute_oof_layout`).
        let mut oof_candidates = OofCandidates::new();

        // Where the content box ends where its `height` or `max-height` puts an end to it, which
        // `line-clamp: auto` keeps the lines within.
        let inset_height = content_box_inset.vertical_components().sum();
        let block_end = known_dimensions
            .height
            .or(node_size.height)
            .or(node_max_size.height)
            .map(|height| (height - inset_height).max(0.0) * scale);
        let room_above = if inline_layout.takes_room_above() {
            let margin = self.nodes[node_id]
                .layout_style()
                .margin()
                .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
            self.room_above(node_id, margin.top, padding.top, container_pb.top, inputs) * scale
        } else {
            0.0
        };
        let area = LineArea {
            width,
            room_above,
            block_end,
            sizes_only: pass != Measure::Layout,
            pass: self.nodes.geometry_generation(),
        };
        let floats = {
            let style = self.nodes[node_id].primary_styles().map(|s| (*s).clone());
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
            crate::text::PERF_COUNTS[1].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            inline_layout.break_lines(&mut self.text, area, style.as_deref(), &mut room);
            room.placed
        };

        let extent = inline_layout.extent(padding.bottom * scale);
        #[cfg_attr(not(feature = "floats"), allow(unused_mut))]
        let mut height = extent.height;

        // A block formatting context's root holds its floats.
        #[cfg(feature = "floats")]
        if is_bfc_root {
            height = floats
                .iter()
                .map(|float| float.bottom)
                .fold(height, f32::max);
        }

        // Note: `width` and `height` are content-box measurements of the inline content.
        // `known_dimensions` must not be substituted in here: those are border-box sizes, and
        // using them for the scrollable overflow rect (which adds padding below) would
        // double-count padding, incorrectly making the container's content overflow it.
        let measured_size = taffy::Size {
            width: width / scale,
            height: height / scale,
        };

        // Unbreakable content (e.g. `white-space: pre` text) may overflow the max advance the
        // lines were broken at. `width` is the advance the lines were broken at, so the actual
        // extent of the line boxes must be taken from the laid-out lines for `content_size`.
        let content_width = f32_max(width, extent.width) / scale;

        let clamped_size = inputs
            .known_dimensions
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

        let container_direction = self.nodes[node_id].layout_style().direction();

        // `align-content` aligns the line boxes (as a single unit) within the content box in
        // the block axis. Floats are positioned relative to the formatting context and are not
        // moved. The offset is stored on the text layout for painting and hit-testing.
        let align_content = BlockContainerStyle::align_content(&self.nodes[node_id].layout_style());
        let block_offset = if align_content.keyword() == taffy::AlignContentKeyword::Normal {
            0.0
        } else {
            let free_space =
                final_size.height - content_box_inset.vertical_axis_sum() - measured_size.height;
            taffy::compute_block_align_content_offset(align_content, free_space)
        };
        let content = (final_size - content_box_inset.sum_axes()).map(|size| size * scale);
        inline_layout.set_frame(block_offset, content);
        let line_box_top = container_pb.top + block_offset;

        // Store sizes and positions of inline boxes. A pass that only sizes the block places
        // nothing.
        let placements = inline_layout
            .placements()
            .filter(|_| pass == Measure::Layout)
            .enumerate();
        let frame_at = PlacedFrame {
            child_inputs,
            container_pb,
            line_box_top,
            content_box_inset,
            final_size,
            container_direction,
            scale,
        };
        for (order, placed) in placements {
            self.place_inline_box(&placed, order as u32, &frame_at, &mut oof_candidates);
        }

        // Each float goes where the lines flowed around it, and into the block formatting context
        // for the blocks after this one.
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
                    .map_or(Clear::None, |(_, clear)| *clear);
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
                // The block's initial letter goes into the context alone: its box is painted from
                // the lines.
                if let Some(node) = TextLayout::float_node(float.key) {
                    self.place_float(
                        node,
                        float,
                        child_inputs,
                        basis,
                        container_pb,
                        scale,
                        &mut oof_candidates,
                    );
                }
            }
        }
        #[cfg(not(feature = "floats"))]
        let _ = floats;

        let line_baseline = |baseline: f32| (baseline / scale) + line_box_top;
        let first_baseline = extent.first_baseline.map(line_baseline);
        let last_baseline = extent.last_baseline.map(line_baseline);

        self.put_inline_layout(node_id, inline_layout);

        let oof_position_inset = taffy::Rect {
            left: border.left,
            right: border.right + scrollbar_gutter.x,
            top: border.top,
            bottom: border.bottom + scrollbar_gutter.y,
        };

        LayoutOutput {
            size: final_size,
            scrollable_overflow_rect: {
                let content_extent = taffy::Size {
                    width: content_width,
                    height: measured_size.height,
                } + padding.sum_axes();
                taffy::Rect {
                    left: 0.0,
                    right: content_extent.width,
                    top: 0.0,
                    bottom: content_extent.height + block_offset.max(0.0),
                }
            },
            baselines: taffy::Baselines {
                first: first_baseline,
                last: last_baseline,
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

    /// Sets where a float the lines flowed around goes, and hands on the absolutely positioned
    /// boxes it holds.
    #[cfg(feature = "floats")]
    #[allow(clippy::too_many_arguments)]
    fn place_float(
        &mut self,
        node: NodeId,
        float: &Floated,
        child_inputs: taffy::LayoutInput,
        basis: f32,
        container_pb: taffy::Rect<f32>,
        scale: f32,
        oof_candidates: &mut OofCandidates,
    ) {
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
        let mut output = self.compute_child_layout(
            crate::taffy_node_id(node),
            float_box_inputs(child_inputs, basis, margin),
        );
        let layout = self.nodes[node].unrounded_layout_mut();
        layout.size = output.size;
        layout.location.x = (float.left / scale) + margin.left + container_pb.left;
        layout.location.y = (float.top / scale) + margin.top + container_pb.top;
        layout.padding = padding;
        layout.border = border;
        layout.margin = margin;
        // The candidates the float holds, from its own corner to this block's.
        if !output.oof_candidates.is_empty() {
            output.oof_candidates.translate(layout.location);
            oof_candidates.append(&mut output.oof_candidates);
        }
    }

    /// Sets where an atomic inline goes, or hands an absolutely positioned box to its containing
    /// block as a candidate placed from its static position, with every one the atomic inline
    /// holds.
    fn place_inline_box(
        &mut self,
        placed: &Placement,
        order: u32,
        at: &PlacedFrame,
        oof_candidates: &mut OofCandidates,
    ) {
        let PlacedFrame {
            child_inputs,
            container_pb,
            line_box_top,
            content_box_inset,
            final_size,
            container_direction,
            scale,
        } = *at;
        let node = &self.nodes[placed.node];
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

        // A float goes where the lines flowed around it.
        #[cfg(feature = "floats")]
        if style.float() != Float::None {
            return;
        }

        let position = style.position();
        let is_absolute = position.is_out_of_flow();
        let item_direction = style.direction();
        // Inline formatting contexts have no `justify-items`/`align-items` for an
        // `auto` self-alignment to defer to, so it behaves as `normal`.
        let justify_self = OofItemStyle::justify_self(&style).unwrap_or(taffy::AlignItems::NORMAL);
        let align_self = OofItemStyle::align_self(&style).unwrap_or(taffy::AlignItems::NORMAL);

        // The static position of an absolutely positioned box depends on the
        // display its hypothetical box would have had (the display specified
        // before position:absolute blockified it): inline-level boxes sit at
        // their position within the line, while block-level boxes start at the
        // content-box left edge of their containing block.
        let is_inline_level =
            style.style.get_box().original_display.outside() == DisplayOutside::Inline;

        // Resolve relative inset offsets against the containing block
        // (the content box of the inline container).
        let container_content_size = final_size - content_box_inset.sum_axes();
        let inset_style = style.inset();
        let inset = taffy::Rect {
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
        let box_inputs = inline_box_inputs(style.size().width, margin, child_inputs);
        drop(style);

        if is_absolute {
            // The static-position rectangle
            // (https://www.w3.org/TR/css-position-3/#staticpos-rect):
            // - An inline-level box's rectangle is zero-width at its position
            //   within the line and spans the line box in the block axis.
            // - A block-level box's rectangle spans the containing block's content
            //   box in the inline axis and is zero-height where the lines say it starts.
            let line_top = (placed.line_top / scale) + line_box_top;
            let line_bottom = (placed.line_bottom / scale) + line_box_top;
            let (inline_area, block_area) = if is_inline_level {
                let x = (placed.x / scale) + container_pb.left;
                (
                    taffy::Line { start: x, end: x },
                    taffy::Line {
                        start: line_top,
                        end: line_bottom,
                    },
                )
            } else {
                let block_start = (placed.block_start / scale) + line_box_top;
                (
                    taffy::Line {
                        start: container_pb.left,
                        end: final_size.width - container_pb.right,
                    },
                    taffy::Line {
                        start: block_start,
                        end: block_start,
                    },
                )
            };
            // An inline-level box's start faces the way the lines say, where they do; in a
            // vertical block the horizontal axis runs across the lines.
            let inline_rtl = match placed.rtl {
                Some(rtl) if is_inline_level => rtl,
                _ => container_direction.is_rtl(),
            };

            oof_candidates.push(OofCandidate {
                node: crate::taffy_node_id(placed.node),
                order,
                position,
                static_position: taffy::Point {
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

        // Re-measure the box to get its border-box size (this hits the layout
        // cache). The size cannot be recovered from the room the line reserves for it,
        // as that is clamped to be non-negative.
        let mut output = self.compute_child_layout(crate::taffy_node_id(placed.node), box_inputs);
        let size = output.size;

        let is_relative = position == taffy::Position::Relative;
        let inset_offset = if is_relative {
            taffy::Point {
                x: if container_direction == Direction::Rtl {
                    inset.right.map(|x| -x).or(inset.left).unwrap_or(0.0)
                } else {
                    inset.left.or(inset.right.map(|x| -x)).unwrap_or(0.0)
                },
                y: inset.top.or(inset.bottom.map(|x| -x)).unwrap_or(0.0),
            }
        } else {
            taffy::Point::ZERO
        };
        // The inline boxes it is in move it as `position: relative` moves them, where the
        // backend paints them moved.
        let shift = TextLayout::inline_shift(
            self,
            placed.node,
            container_content_size,
            container_direction == Direction::Rtl,
        );

        let layout = self.nodes[placed.node].unrounded_layout_mut();
        layout.size = size;
        layout.scrollable_overflow_rect = output.scrollable_overflow_rect;
        layout.location.x =
            (placed.x / scale) + margin.left + container_pb.left + inset_offset.x + shift.x;
        // The lines place the margin box; offset to the border box even when a
        // negative top margin makes it extend above the margin box.
        layout.location.y =
            (placed.top / scale) + margin.top + line_box_top + inset_offset.y + shift.y;
        layout.padding = padding;
        layout.border = border;
        layout.margin = margin;

        // Translate anchors from item-relative to container-relative
        // coordinates and collect candidates bubbled from the box's subtree
        if !output.oof_candidates.is_empty() {
            let location = layout.location;
            output.oof_candidates.translate(location);
            oof_candidates.append(&mut output.oof_candidates);
        }
    }

    /// Lays out an inline formatting context whose lines run down the page: the room along them
    /// is the box's height, and how far they stack across is its width. Taffy has no writing
    /// modes, so the box is sized here in physical terms, and what the lines place is turned onto
    /// the page. The floats of the blocks around it are not in its lines' way: Taffy places them
    /// in horizontal terms.
    fn lay_out_vertical_lines(
        &mut self,
        node_id: NodeId,
        mut inline_layout: Box<TextLayout>,
        frame: Frame,
        lines: LinePass<'_>,
    ) -> LayoutOutput {
        let Frame {
            inputs,
            node_size,
            node_min_size,
            node_max_size,
            padding,
            border,
            scrollbar_gutter,
            container_pb,
            content_box_inset,
            available_space,
            scale,
            ..
        } = frame;
        let LinePass {
            pass,
            child_inputs,
            basis,
            ..
        } = lines;
        let known_dimensions = inputs.known_dimensions;
        let flow = inline_layout.line_flow();
        let margin = self.nodes[node_id]
            .layout_style()
            .margin()
            .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
        let pbh = container_pb.vertical_components().sum() * scale;
        // Its own height, or where it has none the height it has room for, or failing both as
        // long as its longest line. A block in a block container whose lines run the same way
        // stretches into the room; any other box fits its content into it.
        let widths = inline_layout.content_widths();
        let fit = |along: f32| along.min(widths.max).max(widths.min).ceil();
        let along = match known_dimensions.height.or(node_size.height) {
            Some(height) => (height * scale) - pbh,
            None => {
                let room = match available_space.height {
                    AvailableSpace::Definite(height) => Some(height),
                    _ => None,
                };
                // The initial containing block's height, which an orthogonal flow without a
                // definite parent height fits into.
                let icb = self.stylist.device().au_viewport_size().height.to_f32_px();
                // An orthogonal flow whose parent has no definite height has at most the
                // parent's `max-height`, as Chrome takes its fallback inline size.
                let room = match self.parent_max_height(node_id) {
                    Some(cap) => {
                        let cap = (cap.min(icb) - margin.vertical_components().sum()).max(0.0);
                        Some(room.map_or(cap, |room| room.min(cap)))
                    }
                    None => room,
                };
                // Asked for its intrinsic height, it answers its lines' intrinsic length.
                let intrinsic = inputs.run_mode == RunMode::ComputeSize
                    && inputs.axis == RequestedAxis::Vertical;
                match room {
                    None if intrinsic && available_space.height == AvailableSpace::MinContent => {
                        widths.min.ceil()
                    }
                    None if intrinsic => widths.max.ceil(),
                    Some(room) if self.stretches_along(node_id) => (room * scale) - pbh,
                    Some(room) => fit((room * scale) - pbh),
                    // Within a parent of definite height, an orthogonal flow fits into that
                    // height, less its margins, and within any other parent into the initial
                    // containing block.
                    None => match inputs.parent_size.height {
                        Some(height) => {
                            fit((height - margin.vertical_components().sum()) * scale - pbh)
                        }
                        None if self.is_orthogonal(node_id) => {
                            fit((icb - margin.vertical_components().sum()) * scale - pbh)
                        }
                        None => widths.max.ceil(),
                    },
                }
            }
        };
        // The padding at the block's start and end, which a vertical line's over and under sides
        // face: the right and left where the lines stack from the right, the left and right in
        // the others.
        let (start_padding, end_padding) = if flow.stacks_from_right() {
            (padding.right, padding.left)
        } else {
            (padding.left, padding.right)
        };
        let area = LineArea {
            width: along,
            room_above: start_padding * scale,
            block_end: None,
            sizes_only: pass != Measure::Layout,
            pass: self.nodes.geometry_generation(),
        };
        let floats = {
            let style = self.nodes[node_id].primary_styles().map(|s| (*s).clone());
            let mut room = FloatRoom {
                #[cfg(feature = "floats")]
                outer: None,
                #[cfg(not(feature = "floats"))]
                outer: core::marker::PhantomData,
                width: along,
                scale,
                #[cfg(feature = "floats")]
                clears: lines.clears,
                placed: Vec::new(),
            };
            crate::text::PERF_COUNTS[1].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            inline_layout.break_lines(&mut self.text, area, style.as_deref(), &mut room);
            room.placed
        };
        // How far the lines and the floats reach across the block from its start.
        let across = floats
            .iter()
            .map(|float| float.bottom)
            .fold(inline_layout.extent(end_padding * scale).height, f32::max);
        let measured_size = Size {
            width: across.ceil() / scale,
            height: along / scale,
        };
        // A vertical block in a horizontal one is an orthogonal flow: where its width is `auto`
        // it is as wide as its lines stack, as Chrome sizes it, rather than stretched across its
        // container.
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
        let content = (final_size - content_box_inset.sum_axes()).map(|size| size * scale);
        inline_layout.set_frame(0.0, content);

        let mut oof_candidates = OofCandidates::new();
        let container_direction = self.nodes[node_id].layout_style().direction();
        // A pass that only sizes the block places nothing.
        if pass == Measure::Layout {
            let placements: Vec<Placement> = inline_layout.placements().collect();
            let frame_at = PlacedFrame {
                child_inputs,
                container_pb,
                line_box_top: container_pb.top,
                content_box_inset,
                final_size,
                container_direction,
                scale,
            };
            for (order, placed) in placements.iter().enumerate() {
                self.place_inline_box(placed, order as u32, &frame_at, &mut oof_candidates);
            }
            #[cfg(feature = "floats")]
            for float in &floats {
                let Some(node) = TextLayout::float_node(float.key) else {
                    continue;
                };
                let (x, y) = inline_layout
                    .place_on_page([float.left, float.right], [float.top, float.bottom]);
                let turned = super::floats::Floated {
                    left: x,
                    right: x,
                    top: y,
                    bottom: y,
                    ..*float
                };
                self.place_float(
                    node,
                    &turned,
                    child_inputs,
                    basis,
                    container_pb,
                    scale,
                    &mut oof_candidates,
                );
            }
        }
        #[cfg(not(feature = "floats"))]
        let _ = (floats, basis);
        self.put_inline_layout(node_id, inline_layout);

        let oof_position_inset = taffy::Rect {
            left: border.left,
            right: border.right + scrollbar_gutter.x,
            top: border.top,
            bottom: border.bottom + scrollbar_gutter.y,
        };
        let content_extent = measured_size + padding.sum_axes();
        LayoutOutput {
            size: final_size,
            scrollable_overflow_rect: taffy::Rect {
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
        }
    }

    /// Whether `node_id`, whose lines run down the page, stretches along them into the room it
    /// is given: whether it is an in-flow block in a block container whose lines run the same
    /// way.
    fn stretches_along(&self, node_id: NodeId) -> bool {
        let node = &self.nodes[node_id];
        if let Some(style) = node.primary_styles() {
            let display = style.slow_clone_display();
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
                .is_some_and(|style| style.writing_mode.is_vertical())
    }

    /// Whether `node_id`, whose lines run down the page, is an orthogonal flow: whether its
    /// layout parent's lines run across its own.
    fn is_orthogonal(&self, node_id: NodeId) -> bool {
        self.nodes[node_id]
            .layout_parent
            .get()
            .and_then(|parent| self.nodes[parent].primary_styles())
            .is_some_and(|style| !style.writing_mode.is_vertical())
    }

    /// The content height `node_id`'s layout parent may grow to where its own height is `auto`
    /// and its `max-height` a length: that length, no less than its `min-height`, in CSS pixels.
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

    /// The room over the block's first line its ruby annotations and emphasis marks may take
    /// before the line moves down for them, in CSS pixels, as Chrome's block layout lends it
    /// (`ComputeInitialBlockStartAnnotationSpace`): the block's padding over the line, and where
    /// no border is in the way, the margin collapsed between it and the block before it, that
    /// block's end padding, and the room that block's last line left under its content.
    fn room_above(
        &self,
        node_id: NodeId,
        margin_top: f32,
        padding_top: f32,
        padding_border_top: f32,
        inputs: taffy::LayoutInput,
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
            .map_or(0.0, |text| text.room_below() / self.viewport.scale());
        room + padding + lent.max(0.0)
    }

    /// The in-flow block laid out before `node_id` among its layout siblings, if there is one.
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
                    && !computed.slow_clone_position().is_absolutely_positioned()
                    && matches!(
                        computed.slow_clone_float(),
                        style::values::computed::Float::None
                    )
            })
        })
    }

    /// Hands the inline layout back to its node.
    pub(crate) fn put_inline_layout(&mut self, node_id: NodeId, inline_layout: Box<TextLayout>) {
        self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .inline_layout_data = Some(inline_layout);
    }

    /// Lays out each atomic inline of the inline formatting context rooted at `node_id` as its
    /// line holds it, and each float as the lines flow around it: its border box, and an atomic
    /// inline's baseline down from its top. Absolutely positioned boxes take no room in the lines,
    /// and are not measured.
    ///
    /// A float is sized in the room beside nothing, less its margins, `basis` being the containing
    /// block's inline size.
    ///
    /// `pass` says how far each box is laid out: where its baseline is not read or the pass needs
    /// no baseline, it is only measured, along the line alone in an inline-size pass.
    /// `child_inputs` are the inputs of a full layout. Lines that run down the page, `vertical`,
    /// read no baseline.
    ///
    /// `baseline-source: auto` is the last baseline for an inline-block and the first otherwise.
    /// An inline-block's last baseline is its last line box's, and a flex, grid or table box's
    /// first baseline its first; a replaced element, or an inline-block that clips its overflow
    /// and takes its last baseline, has none, and sits on its margin box's bottom.
    fn measure_inline_boxes(
        &mut self,
        node_id: NodeId,
        child_inputs: taffy::LayoutInput,
        pass: Measure,
        basis: f32,
        vertical: bool,
    ) -> Vec<BoxMeasure> {
        let children = self.nodes[node_id].layout_children.borrow().clone();
        let mut sizes = Vec::with_capacity(children.as_ref().map_or(0, |children| children.len()));
        let rtl = self.nodes[node_id].primary_styles().is_some_and(|style| {
            style.get_inherited_box().direction == style::computed_values::direction::T::Rtl
        });
        for node in children.iter().flatten().copied() {
            let held = &self.nodes[node];
            let style = self.child_layout_style(held);
            let margin = style
                .margin()
                .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);

            #[cfg(feature = "floats")]
            let is_floated = style.float().is_floated();
            #[cfg(not(feature = "floats"))]
            let is_floated = false;

            if style.position().is_out_of_flow() {
                continue;
            }
            if is_floated {
                // `inline-start` and `inline-end` are the containing block's sides, as its
                // direction has them.
                let float_side = match (held.primary_styles().map(|s| s.slow_clone_float()), rtl) {
                    (Some(StyloFloat::Right), _)
                    | (Some(StyloFloat::InlineStart), true)
                    | (Some(StyloFloat::InlineEnd), false) => FloatSide::Right,
                    _ => FloatSide::Left,
                };
                let pass_inputs = match pass {
                    Measure::Layout => child_inputs,
                    Measure::Sizes => taffy::LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        ..child_inputs
                    },
                    Measure::InlineSizes => taffy::LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        axis: RequestedAxis::Horizontal,
                        ..child_inputs
                    },
                };
                drop(style);
                let output = self.compute_child_layout(
                    crate::taffy_node_id(node),
                    float_box_inputs(pass_inputs, basis, margin),
                );
                sizes.push(BoxMeasure {
                    node,
                    size: output.size,
                    margin,
                    baseline: None,
                    float: Some(float_side),
                });
                continue;
            }
            let replaced = held
                .data
                .downcast_element()
                .is_some_and(|element| is_replaced_element(&element.name.local));
            let inside = held.display_style().map(|display| display.inside());
            let source = held
                .primary_styles()
                .map_or(BaselineSource::Auto, |computed| {
                    computed.slow_clone_baseline_source()
                });
            let uses_last = match source {
                BaselineSource::First => false,
                BaselineSource::Last => true,
                BaselineSource::Auto => {
                    matches!(inside, Some(DisplayInside::Flow | DisplayInside::FlowRoot))
                }
            };
            // Only `baseline-source: auto` lets an inline-block that clips sit on its margin
            // box's bottom.
            let clips = source == BaselineSource::Auto && {
                let overflow = style.overflow();
                overflow.x != Overflow::Visible || overflow.y != Overflow::Visible
            };
            let has_baseline = !replaced && !(uses_last && clips);
            let reads_baseline = has_baseline
                && !vertical
                && held
                    .primary_styles()
                    .is_none_or(|computed| reads_own_baseline(&computed));
            let pass_inputs = match pass {
                Measure::Layout => child_inputs,
                Measure::Sizes if reads_baseline => child_inputs,
                Measure::Sizes => taffy::LayoutInput {
                    run_mode: RunMode::ComputeSize,
                    ..child_inputs
                },
                Measure::InlineSizes => taffy::LayoutInput {
                    run_mode: RunMode::ComputeSize,
                    axis: RequestedAxis::Horizontal,
                    ..child_inputs
                },
            };
            let box_inputs = inline_box_inputs(style.size().width, margin, pass_inputs);
            drop(style);

            let output = self.compute_child_layout(crate::taffy_node_id(node), box_inputs);
            let reads_baseline = reads_baseline && pass_inputs.run_mode == RunMode::PerformLayout;
            let baseline = if !reads_baseline {
                None
            } else if uses_last {
                // An inline formatting context's own baseline is its layout's; a block
                // container's is found among its children.
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
            };
            sizes.push(BoxMeasure {
                node,
                size: output.size,
                margin,
                baseline,
                float: None,
            });
        }
        sizes
    }

    /// An inline-block's baseline, its last line box's, down from the border box's top of
    /// `node`, a block container laid out already, in CSS pixels. The last in-flow child in its
    /// writing mode that has one gives it, and a child that is a scroll container gives its margin
    /// box's bottom edge.
    fn last_line_baseline(&self, node: NodeId) -> LastBaseline {
        let held = &self.nodes[node];
        let vertical = held
            .primary_styles()
            .is_some_and(|computed| computed.writing_mode.is_vertical());
        if let Some(text) = held
            .element_data()
            .and_then(|element| element.inline_layout_data.as_ref())
        {
            // Lines that run down the page have no baseline across them.
            if vertical {
                return LastBaseline::Unknown;
            }
            return match text.last_line_baseline() {
                LastBaseline::At(baseline) => {
                    let edges = held.unrounded_layout();
                    LastBaseline::At(
                        (baseline / text.scale()) + edges.border.top + edges.padding.top,
                    )
                }
                other => other,
            };
        }
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
                || computed.slow_clone_position().is_absolutely_positioned()
                || !matches!(
                    computed.slow_clone_float(),
                    style::values::computed::Float::None
                )
            {
                continue;
            }
            let scrolls = !matches!(
                computed.slow_clone_overflow_x(),
                style::values::computed::Overflow::Visible
            ) || !matches!(
                computed.slow_clone_overflow_y(),
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
}

/// Whether the lines read the own baseline of an atomic inline set in `style` to set it on its
/// line. A pass that sets the lines lays such a box out to measure it, since Taffy gives a
/// baseline only with a layout.
///
/// A box aligned by its edges, `top`, `bottom`, `text-top` or `text-bottom`, is not: its margin
/// box's edge goes where the alignment says, wherever its baseline is.
fn reads_own_baseline(style: &style::properties::ComputedValues) -> bool {
    use style::values::computed::AlignmentBaseline;
    use style::values::computed::length_percentage::Unpacked;
    use style::values::generics::box_::{BaselineShiftKeyword, GenericBaselineShift};
    let box_style = style.get_box();
    match box_style.slow_clone_baseline_shift() {
        GenericBaselineShift::Keyword(BaselineShiftKeyword::Top | BaselineShiftKeyword::Bottom) => {
            return false;
        }
        GenericBaselineShift::Keyword(_) => return true,
        GenericBaselineShift::Length(shift) => match shift.unpack() {
            Unpacked::Length(length) if length.px() != 0.0 => return true,
            Unpacked::Percentage(percentage) if percentage.0 != 0.0 => return true,
            _ => {}
        },
    }
    !matches!(
        box_style.slow_clone_alignment_baseline(),
        AlignmentBaseline::TextTop | AlignmentBaseline::TextBottom
    )
}

/// What the pass that lays out an inline formatting context measured its boxes for.
#[derive(Clone, Copy)]
struct LinePass<'a> {
    pass: Measure,
    /// The inputs of a full layout of an atomic inline or a float.
    child_inputs: taffy::LayoutInput,
    /// The containing block's inline size, which a float's room is taken of.
    basis: f32,
    /// Each float's `clear`, by node.
    #[cfg(feature = "floats")]
    clears: &'a [(u64, Clear)],
    #[cfg(not(feature = "floats"))]
    #[allow(dead_code)]
    clears: core::marker::PhantomData<&'a ()>,
}

/// Where the boxes an inline formatting context's lines hold are placed from.
#[derive(Clone, Copy)]
struct PlacedFrame {
    /// The inputs of a full layout of an atomic inline.
    child_inputs: taffy::LayoutInput,
    /// The inline root's padding and border.
    container_pb: taffy::Rect<f32>,
    /// Where the line boxes start, down from the border box's top, in CSS pixels.
    line_box_top: f32,
    content_box_inset: taffy::Rect<f32>,
    /// The inline root's border box.
    final_size: Size<f32>,
    container_direction: Direction,
    /// Device pixels per CSS pixel.
    scale: f32,
}
