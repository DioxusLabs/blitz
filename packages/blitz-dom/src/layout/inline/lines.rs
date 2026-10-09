//! Measuring an inline formatting context's boxes, breaking its lines and placing what they hold,
//! for every text backend.

use blitz_traits::node_id::NodeId;
use style::values::specified::box_::BaselineSource;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use taffy::{
    AvailableSpace, AxisStaticPosition, BlockContainerStyle, BlockContext, CollapsibleMarginSet,
    CoreStyle as _, Direction, LayoutOutput, LayoutPartialTree as _, MaybeMath as _,
    MaybeResolve as _, OofCandidate, OofCandidates, OofItemStyle, OofPositioningArea, Overflow,
    Point, RequestedAxis, ResolveOrZero as _, Size,
};

#[cfg(feature = "floats")]
use taffy::{BlockItemStyle as _, Clear, Float};

use super::floats::TaffyFloats;
#[cfg(feature = "floats")]
use super::subtract_margins;
use super::{Frame, f32_max, inline_box_inputs};
use crate::layout::LayoutPassState;
use crate::layout::replaced::is_replaced_element;
use crate::layout::resolve_calc_value;
use crate::node::TextLayout;
use crate::text::{BoxMeasure, InlineLayoutEngine as _, InlineText as _, LastBaseline};

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

        let sizes = self.measure_inline_boxes(node_id, inputs.parent_size, child_inputs);
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
                let content_sizes = inline_layout.content_widths();
                let min_content_width = content_sizes.min;
                let max_content_width = content_sizes.max;

                #[cfg(feature = "floats")]
                let float_width = self.float_widths(
                    node_id,
                    available_space.width,
                    inputs.parent_size,
                    child_inputs,
                ) * scale;
                #[cfg(not(feature = "floats"))]
                let float_width = 0.0;

                let computed_width = match available_space.width {
                    AvailableSpace::MinContent => min_content_width.max(float_width),
                    AvailableSpace::MaxContent => max_content_width + float_width,
                    AvailableSpace::Definite(limit) => (limit * scale)
                        .min(max_content_width + float_width)
                        .max(min_content_width),
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
        #[cfg(feature = "floats")]
        let block_ctx = &mut block_ctx;

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

        {
            let style = self.nodes[node_id].primary_styles().map(|s| (*s).clone());
            let mut floats = TaffyFloats {
                state: self,
                block_ctx,
                scale,
                float_inputs: child_inputs,
                parent_size: inputs.parent_size,
                container_pb,
                oof_candidates: &mut oof_candidates,
            };
            inline_layout.break_lines(width, style.as_deref(), &mut floats);
        }

        // Propagate the height consumed by floats placed within this container to the
        // enclosing block context (so that the BFC root can contain them)
        #[cfg(feature = "floats")]
        let float_height_contribution = block_ctx.floated_content_height_contribution();
        #[cfg(feature = "floats")]
        outer_block_ctx.add_child_floated_content_height_contribution(
            container_pb.top + float_height_contribution,
        );

        let extent = inline_layout.extent();
        #[cfg_attr(not(feature = "floats"), allow(unused_mut))]
        let mut height = extent.height;

        #[cfg(feature = "floats")]
        {
            if is_bfc_root {
                height = height.max(float_height_contribution * scale)
            };
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
        inline_layout.set_block_offset(block_offset);
        let line_box_top = container_pb.top + block_offset;

        // Store sizes and positions of inline boxes
        for (order, placed) in inline_layout.placements().enumerate() {
            let order = order as u32;
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
                let layout = self.nodes[placed.node].unrounded_layout_mut();
                layout.padding = padding;
                layout.border = border;
                layout.margin = margin;
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

    /// Hands the inline layout back to its node.
    pub(crate) fn put_inline_layout(&mut self, node_id: NodeId, inline_layout: Box<TextLayout>) {
        self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .inline_layout_data = Some(inline_layout);
    }

    /// Lays out each atomic inline of the inline formatting context rooted at `node_id` as its
    /// line holds it: its border box, and its baseline down from its top. Absolutely positioned
    /// boxes and floats take no room in the lines, and are not measured.
    ///
    /// `baseline-source: auto` is the last baseline for an inline-block and the first otherwise.
    /// An inline-block's last baseline is its last line box's, and a flex, grid or table box's
    /// first baseline its first; a replaced element, or an inline-block that clips its overflow
    /// and takes its last baseline, has none, and sits on its margin box's bottom.
    fn measure_inline_boxes(
        &mut self,
        node_id: NodeId,
        parent_size: Size<Option<f32>>,
        child_inputs: taffy::LayoutInput,
    ) -> Vec<BoxMeasure> {
        let children = self.nodes[node_id].layout_children.borrow().clone();
        let mut sizes = Vec::with_capacity(children.as_ref().map_or(0, |children| children.len()));
        for node in children.iter().flatten().copied() {
            let held = &self.nodes[node];
            let style = self.child_layout_style(held);
            let margin = style
                .margin()
                .resolve_or_zero(parent_size, resolve_calc_value);

            #[cfg(feature = "floats")]
            let is_floated = style.float().is_floated();
            #[cfg(not(feature = "floats"))]
            let is_floated = false;

            if style.position().is_out_of_flow() || is_floated {
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
                    computed.clone_baseline_source()
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
            let box_inputs = inline_box_inputs(style.size().width, margin, child_inputs);
            drop(style);

            let output = self.compute_child_layout(crate::taffy_node_id(node), box_inputs);
            let reads_baseline =
                has_baseline && box_inputs.run_mode == taffy::RunMode::PerformLayout;
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

    /// How wide the floats of the inline formatting context rooted at `node_id` make its content,
    /// in CSS pixels, under an intrinsic sizing constraint: the widest of them at min-content, and
    /// at max-content those that share a band side by side.
    #[cfg(feature = "floats")]
    fn float_widths(
        &mut self,
        node_id: NodeId,
        available_width: AvailableSpace,
        parent_size: Size<Option<f32>>,
        child_inputs: taffy::LayoutInput,
    ) -> f32 {
        if let AvailableSpace::Definite(_) = available_width {
            return 0.0;
        }
        let children = self.nodes[node_id].layout_children.borrow().clone();
        // When computing a max-content size the available width is effectively
        // infinite, so floats never wrap onto a new "band" due to a lack of
        // horizontal space. They only move below preceding floats when the `clear`
        // property forces them to.
        //
        // Floats that share a band sit side-by-side and so their widths sum, whereas
        // floats pushed onto a new band (via `clear`) stack vertically and so we
        // take the maximum extent across bands rather than summing.
        let mut left_band: f32 = 0.0;
        let mut right_band: f32 = 0.0;
        let mut width: f32 = 0.0;
        for node in children.iter().flatten().copied() {
            let (float, clear, margin) = {
                let style = self.child_layout_style(&self.nodes[node]);
                (
                    style.float(),
                    style.clear(),
                    style
                        .margin()
                        .resolve_or_zero(parent_size, resolve_calc_value),
                )
            };
            if !float.is_floated() {
                continue;
            }
            let output = self.compute_child_layout(
                crate::taffy_node_id(node),
                subtract_margins(child_inputs, margin),
            );
            let box_width = output.size.width + margin.left + margin.right;
            if available_width == AvailableSpace::MinContent {
                width = width.max(box_width);
                continue;
            }
            if matches!(clear, Clear::Left | Clear::Both) {
                left_band = 0.0;
            }
            if matches!(clear, Clear::Right | Clear::Both) {
                right_band = 0.0;
            }
            match float {
                Float::Left => left_band += box_width,
                Float::Right => right_band += box_width,
                Float::None => {}
            }
            width = width.max(left_band + right_band);
        }
        width
    }
}
