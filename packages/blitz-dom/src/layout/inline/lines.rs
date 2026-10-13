//! Measuring an inline formatting context's boxes, breaking its lines and placing what they hold,
//! for every text backend.

use blitz_traits::node_id::NodeId;
use style::values::computed::{BaselineSource, Contain, Float as StyloFloat};
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use taffy::{
    AvailableSpace, AxisStaticPosition, BlockContainerStyle, BlockContext, CollapsibleMarginSet,
    CoreStyle as _, Direction, LayoutOutput, LayoutPartialTree as _, MaybeMath as _,
    MaybeResolve as _, OofCandidate, OofCandidates, OofItemStyle, OofPositioningArea, Overflow,
    Point, RequestedAxis, ResolveOrZero as _, RunMode, Size,
};

#[cfg(feature = "floats")]
use taffy::{BlockItemStyle as _, Clear, Float};

use stylo_taffy::StyleFlags;

#[cfg(feature = "floats")]
use super::floats::TaffyFloats;
use super::{Frame, float_box_inputs, inline_box_inputs};
use crate::layout::LayoutPassState;
use crate::layout::resolve_calc_value;
use crate::node::TextLayout;
#[cfg(not(feature = "floats"))]
use crate::text::NoLineExclusions;
use crate::text::{BoxMeasure, FloatClear, FloatSide, InlineLayoutEngine as _};

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
            suppresses_baseline,
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

        // What the pass needs of the atomic inlines, which says how far each is laid out to
        // measure it. A pass that places them lays them out.
        let pass = match inputs.run_mode {
            RunMode::ComputeSize if inputs.axis == RequestedAxis::Horizontal => {
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
        let sizes = self.measure_inline_boxes(node_id, child_inputs, pass);
        inline_layout.prepare(
            &sizes,
            self.nodes[node_id]
                .primary_styles()
                .as_deref()
                .map(|s| &**s),
        );

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
        let outer_block_ctx = block_ctx;
        #[cfg(feature = "floats")]
        let mut block_ctx =
            outer_block_ctx.sub_context(container_pb.top, [container_pb.left, container_pb.right]);
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

        let style = self.nodes[node_id].primary_styles().map(|s| (*s).clone());
        // The floats the lines place are read back from the block formatting context.
        #[cfg(feature = "floats")]
        let float_mark = block_ctx.float_mark();
        #[cfg(feature = "floats")]
        inline_layout.break_lines(
            width,
            style.as_deref(),
            &mut TaffyFloats {
                ctx: &mut block_ctx,
                scale,
            },
        );
        #[cfg(not(feature = "floats"))]
        inline_layout.break_lines(width, style.as_deref(), &mut NoLineExclusions);
        drop(style);

        let extent = inline_layout.extent();
        #[cfg_attr(not(feature = "floats"), allow(unused_mut))]
        let mut height = extent.height;

        #[cfg(feature = "floats")]
        let float_height_contribution = block_ctx.floated_content_height_contribution();
        // A block formatting context's root holds its floats.
        #[cfg(feature = "floats")]
        if is_bfc_root {
            height = height.max(float_height_contribution * scale);
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
        let content_width = width.max(extent.width) / scale;

        let clamped_size = inputs
            .known_dimensions
            .or(node_size)
            .unwrap_or(measured_size + content_box_inset.sum_axes())
            .maybe_clamp(node_min_size, node_max_size);
        let final_size = Size {
            width: clamped_size.width,
            height: clamped_size.height.max(
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
        inline_layout.set_block_offset(block_offset);
        let line_box_top = container_pb.top + block_offset;

        // Store sizes and positions of inline boxes. A pass that only sizes the block places
        // nothing.
        let mut span_cb_flags = StyleFlags::empty();
        let placements = inline_layout
            .placements()
            .filter(|_| pass == Measure::Layout)
            .enumerate();
        for (order, placed) in placements {
            let order = order as u32;
            if inputs.run_mode == RunMode::PerformLayout {
                span_cb_flags |= self.inline_span_cb_flags(node_id, placed.node);
            }
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

            #[cfg(feature = "floats")]
            let is_floated = style.float() != Float::None;
            #[cfg(not(feature = "floats"))]
            let is_floated = false;

            let position = style.position();
            let is_absolute = position.is_out_of_flow();
            let item_direction = style.direction();
            // Inline formatting contexts have no `justify-items`/`align-items` for an
            // `auto` self-alignment to defer to, so it behaves as `normal`.
            let justify_self =
                OofItemStyle::justify_self(&style).unwrap_or(taffy::AlignItems::NORMAL);
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
                            container_direction.is_rtl(),
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
            } else if is_floated {
                // Placed below, where the lines flowed around it.
            } else {
                // Re-measure the box to get its border-box size (this hits the layout
                // cache). The size cannot be recovered from the room the line reserves for it,
                // as that is clamped to be non-negative.
                let mut output =
                    self.compute_child_layout(crate::taffy_node_id(placed.node), box_inputs);
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

                let layout = self.nodes[placed.node].unrounded_layout_mut();
                layout.size = size;
                layout.scrollable_overflow_rect = output.scrollable_overflow_rect;
                layout.location.x =
                    (placed.x / scale) + margin.left + container_pb.left + inset_offset.x;
                // The lines place the margin box; offset to the border box even when a
                // negative top margin makes it extend above the margin box.
                layout.location.y =
                    (placed.top / scale) + margin.top + line_box_top + inset_offset.y;
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
        }

        // Each float goes where Taffy placed it as the lines reached it.
        #[cfg(feature = "floats")]
        if inputs.run_mode == RunMode::PerformLayout {
            for (key, pos) in block_ctx.floats_placed_since(float_mark) {
                self.place_float(
                    NodeId::from_u64(key),
                    pos,
                    child_inputs,
                    container_pb,
                    &mut oof_candidates,
                );
            }
        }

        // Propagate the height consumed by floats placed within this container to the
        // enclosing block context (so that the BFC root can contain them)
        #[cfg(feature = "floats")]
        outer_block_ctx.add_child_floated_content_height_contribution(
            container_pb.top + float_height_contribution,
        );

        let line_baseline = |baseline: f32| (baseline / scale) + line_box_top;
        let first_baseline = extent
            .first_baseline
            .filter(|_| !suppresses_baseline)
            .map(line_baseline);
        let last_baseline = extent
            .last_baseline
            .filter(|_| !suppresses_baseline)
            .map(line_baseline);

        if inputs.run_mode == RunMode::PerformLayout {
            inline_layout.span_cb_flags = span_cb_flags;
        }

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
    fn place_float(
        &mut self,
        node: NodeId,
        pos: taffy::Point<f32>,
        child_inputs: taffy::LayoutInput,
        container_pb: taffy::Rect<f32>,
        oof_candidates: &mut OofCandidates,
    ) {
        let (width_style, margin, padding, border) = {
            let style = self.child_layout_style(&self.nodes[node]);
            (
                style.size().width,
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
            float_box_inputs(width_style, margin, child_inputs),
        );
        let layout = self.nodes[node].unrounded_layout_mut();
        layout.size = output.size;
        layout.scrollable_overflow_rect = output.scrollable_overflow_rect;
        layout.location.x = pos.x + margin.left + container_pb.left;
        layout.location.y = pos.y + margin.top + container_pb.top;
        layout.padding = padding;
        layout.border = border;
        layout.margin = margin;
        // The candidates the float holds, from its own corner to this block's.
        if !output.oof_candidates.is_empty() {
            output.oof_candidates.translate(layout.location);
            oof_candidates.append(&mut output.oof_candidates);
        }
    }

    /// Hands the inline layout back to its node.
    fn put_inline_layout(&mut self, node_id: NodeId, inline_layout: Box<TextLayout>) {
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
    /// A float is sized in the room beside nothing, less its margins.
    ///
    /// `pass` says how far each box is laid out: where its baseline is not read or the pass needs
    /// no baseline, it is only measured, along the line alone in an inline-size pass.
    /// `child_inputs` are the inputs of a full layout.
    ///
    /// `baseline-source: auto` is the last baseline for an inline-block and the first otherwise.
    /// An inline-block's last baseline is its last line box's, and a flex, grid or table box's
    /// first baseline its first; a replaced element, or an inline-block other than a button that
    /// clips its overflow and takes its last baseline, has none, and sits on its margin box's
    /// bottom.
    fn measure_inline_boxes(
        &mut self,
        node_id: NodeId,
        child_inputs: taffy::LayoutInput,
        pass: Measure,
    ) -> Vec<BoxMeasure> {
        // Taken for the loop, which lays the children out, and put back after it.
        let children = self.nodes[node_id].layout_children.borrow_mut().take();
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
                let float_side = match (held.primary_styles().map(|s| s.clone_float()), rtl) {
                    (Some(StyloFloat::Right), _)
                    | (Some(StyloFloat::InlineStart), true)
                    | (Some(StyloFloat::InlineEnd), false) => FloatSide::Right,
                    _ => FloatSide::Left,
                };
                #[cfg(feature = "floats")]
                let clear = match style.clear() {
                    Clear::None => FloatClear::None,
                    Clear::Left => FloatClear::Left,
                    Clear::Right => FloatClear::Right,
                    Clear::Both => FloatClear::Both,
                };
                #[cfg(not(feature = "floats"))]
                let clear = FloatClear::None;
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
                let box_inputs = float_box_inputs(style.size().width, margin, pass_inputs);
                drop(style);
                let output = self.compute_child_layout(crate::taffy_node_id(node), box_inputs);
                sizes.push(BoxMeasure {
                    node,
                    size: output.size,
                    margin,
                    baseline: None,
                    float: Some(float_side),
                    clear,
                });
                continue;
            }
            // `baseline-source: auto` selects the last baseline for inline-blocks and the first
            // for other atomic inlines. A block-axis scroll-container inline-block (except buttons)
            // instead uses its bottom margin edge, but only with `auto` (css-align-3 §9.1).
            // `overflow: clip` is not a scroll container. Exported scroll-container baselines
            // are clamped to the border box.
            // A layout-contained box is treated as having no baseline (css-contain-1 §3.3).
            let overflow = style.overflow();
            let box_style = style.style.get_box();
            let is_flow = matches!(
                style.effective_display().inside(),
                DisplayInside::Flow | DisplayInside::FlowRoot
            );
            let baseline_source = box_style.clone_baseline_source();
            let uses_last_baseline = match baseline_source {
                BaselineSource::Auto => is_flow,
                BaselineSource::First => false,
                BaselineSource::Last => true,
            };
            let is_scroll_container = !matches!(overflow.y, Overflow::Visible | Overflow::Clip);
            let is_block_axis_scroll_container = is_flow
                && is_scroll_container
                && !style
                    .flags
                    .intersects(StyleFlags::IS_BUTTON | StyleFlags::IS_BUTTON_INPUT);
            let contain_layout = box_style.clone_contain().contains(Contain::LAYOUT);
            let exports_baseline = !(baseline_source == BaselineSource::Auto
                && is_block_axis_scroll_container)
                && !contain_layout;
            let reads_baseline = exports_baseline
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
            let baseline = if exports_baseline {
                let baseline = if uses_last_baseline {
                    output.baselines.last.or(output.baselines.first)
                } else {
                    output.baselines.first.or(output.baselines.last)
                };
                baseline.map(|baseline| {
                    if is_scroll_container {
                        baseline.clamp(0.0, output.size.height)
                    } else {
                        baseline
                    }
                })
            } else {
                None
            };
            sizes.push(BoxMeasure {
                node,
                size: output.size,
                margin,
                baseline,
                float: None,
                clear: FloatClear::None,
            });
        }
        *self.nodes[node_id].layout_children.borrow_mut() = children;
        sizes
    }
}

/// What a pass needs of an inline formatting context's atomic inlines, which says how far each is
/// laid out to measure it.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum Measure {
    /// How far each reaches along the line: an intrinsic inline-size pass.
    InlineSizes,
    /// Each one's size, and the baselines the lines read: a pass that sets the lines to size the
    /// block but places nothing.
    Sizes,
    /// Each one laid out, as the pass that places them lays them out.
    Layout,
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
    match box_style.clone_baseline_shift() {
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
        box_style.clone_alignment_baseline(),
        AlignmentBaseline::TextTop | AlignmentBaseline::TextBottom
    )
}
