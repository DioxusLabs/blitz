use parley::{AlignmentOptions, IndentOptions, PositionedInlineBox};
use style::values::{computed::CSSPixelLength, generics::text::GenericTextIndent};
use taffy::{
    AvailableSpace, BlockContext, BlockFormattingContext, BoxSizing, CollapsibleMarginSet,
    CoreStyle as _, Direction, LayoutInput, LayoutOutput, LayoutPartialTree as _, MaybeMath as _,
    MaybeResolve as _, NodeId, Overflow, Point, Position, ResolveOrZero as _, RunMode, Size,
    SizingMode,
};

#[cfg(feature = "floats")]
use parley::YieldData;
#[cfg(feature = "floats")]
use taffy::{Clear, Float, prelude::TaffyMaxContent};

use super::resolve_calc_value;
use crate::BaseDocument;

impl BaseDocument {
    pub(crate) fn compute_inline_layout(
        &mut self,
        node_id: usize,
        inputs: taffy::tree::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::LayoutOutput {
        let LayoutInput {
            known_dimensions,
            parent_size,
            run_mode,
            ..
        } = inputs;
        let style = &self.nodes[node_id].style;

        // Pull these out earlier to avoid borrowing issues
        let is_scroll_container =
            style.overflow.x.is_scroll_container() || style.overflow.y.is_scroll_container();
        let padding = style
            .padding()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let border = style
            .border()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let padding_border_size = (padding + border).sum_axes();
        let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox {
            padding_border_size
        } else {
            Size::ZERO
        };

        // Resolve node's preferred/min/max sizes (width/heights) against the available space (percentages resolve to pixel values)
        // For ContentSize mode, we pretend that the node has no size styles as these should be ignored.
        let (clamped_style_size, min_size, max_size, _aspect_ratio) = match inputs.sizing_mode {
            SizingMode::ContentSize => {
                let node_size = known_dimensions;
                let node_min_size = Size::NONE;
                let node_max_size = Size::NONE;
                (node_size, node_min_size, node_max_size, None)
            }
            SizingMode::InherentSize => {
                let aspect_ratio = style.aspect_ratio();
                let style_size = style
                    .size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_apply_aspect_ratio(aspect_ratio)
                    .maybe_add(box_sizing_adjustment);
                let style_min_size = style
                    .min_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_apply_aspect_ratio(aspect_ratio)
                    .maybe_add(box_sizing_adjustment);
                let style_max_size = style
                    .max_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment);

                let node_size =
                    known_dimensions.or(style_size.maybe_clamp(style_min_size, style_max_size));
                (node_size, style_min_size, style_max_size, aspect_ratio)
            }
        };

        // If both min and max in a given axis are set and max <= min then this determines the size in that axis
        let min_max_definite_size = min_size.zip_map(max_size, |min, max| match (min, max) {
            (Some(min), Some(max)) if max <= min => Some(min),
            _ => None,
        });

        let styled_based_known_dimensions = known_dimensions
            .or(min_max_definite_size)
            .or(clamped_style_size)
            .maybe_max(padding_border_size);

        // Short-circuit layout if the container's size is fully determined by the container's size and the run mode
        // is ComputeSize (and thus the container's size is all that we're interested in)
        if run_mode == RunMode::ComputeSize {
            if let Size {
                width: Some(width),
                height: Some(height),
            } = styled_based_known_dimensions
            {
                return LayoutOutput::from_outer_size(Size { width, height });
            }
        }

        // Unwrap the block formatting context if one was passed, or else create a new one
        match block_ctx {
            Some(inherited_bfc) if !is_scroll_container => self.compute_inline_layout_inner(
                node_id,
                LayoutInput {
                    known_dimensions: styled_based_known_dimensions,
                    ..inputs
                },
                inherited_bfc,
            ),
            _ => {
                let mut root_bfc = BlockFormattingContext::new();
                let mut root_ctx = root_bfc.root_block_context();
                self.compute_inline_layout_inner(
                    node_id,
                    LayoutInput {
                        known_dimensions: styled_based_known_dimensions,
                        ..inputs
                    },
                    &mut root_ctx,
                )
            }
        }
    }

    fn compute_inline_layout_inner(
        &mut self,
        node_id: usize,
        inputs: taffy::tree::LayoutInput,
        block_ctx: &mut BlockContext<'_>,
    ) -> taffy::LayoutOutput {
        let scale = self.viewport.scale();
        let LayoutInput {
            known_dimensions,
            parent_size,
            available_space,
            sizing_mode,
            ..
        } = inputs;

        // Take inline layout to satisfy borrow checker
        let mut inline_layout = self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .take_inline_layout()
            .unwrap();

        let style = &self.nodes[node_id].style;

        // Note: both horizontal and vertical percentage padding/borders are resolved against the container's inline size (i.e. width).
        // This is not a bug, but is how CSS is specified (see: https://developer.mozilla.org/en-US/docs/Web/CSS/padding#values)
        let margin = style
            .margin()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let padding = style
            .padding()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let border = style
            .border()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let container_pb = padding + border;
        let pb_sum = container_pb.sum_axes();
        let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox {
            pb_sum
        } else {
            Size::ZERO
        };

        // Scrollbar gutters are reserved when the `overflow` property is set to `Overflow::Scroll`.
        // However, the axis are switched (transposed) because a node that scrolls vertically needs
        // *horizontal* space to be reserved for a scrollbar
        let scrollbar_gutter = style.overflow().transpose().map(|overflow| match overflow {
            Overflow::Scroll => style.scrollbar_width(),
            _ => 0.0,
        });
        // TODO: make side configurable based on the `direction` property
        let mut content_box_inset = container_pb;
        content_box_inset.right += scrollbar_gutter.x;
        content_box_inset.bottom += scrollbar_gutter.y;

        let has_styles_preventing_being_collapsed_through = !style.is_block()
            || style.overflow().x.is_scroll_container()
            || style.overflow().y.is_scroll_container()
            || style.position() == Position::Absolute
            || padding.top > 0.0
            || padding.bottom > 0.0
            || border.top > 0.0
            || border.bottom > 0.0;
        // || matches!(node_size.height, Some(h) if h > 0.0)
        // || matches!(node_min_size.height, Some(h) if h > 0.0)
        // || !inline_layout.text.is_empty();
        // || !inline_layout.layout.inline_boxes().is_empty();

        // Short circuit if inline context contains no text or inline boxes
        if !has_styles_preventing_being_collapsed_through
            && inline_layout.text.is_empty()
            && inline_layout.layout.inline_boxes().is_empty()
        {
            // Put layout back
            self.nodes[node_id]
                .data
                .downcast_element_mut()
                .unwrap()
                .inline_layout_data = Some(inline_layout);
            return LayoutOutput::from_outer_size(
                Size::ZERO.maybe_max(container_pb.sum_axes().map(Some)),
            );
        }

        // Resolve node's preferred/min/max sizes (width/heights) against the available space (percentages resolve to pixel values)
        // For ContentSize mode, we pretend that the node has no size styles as these should be ignored.
        let (node_size, node_min_size, node_max_size, aspect_ratio) = match sizing_mode {
            SizingMode::ContentSize => {
                let node_size = known_dimensions;
                let node_min_size = Size::NONE;
                let node_max_size = Size::NONE;
                (node_size, node_min_size, node_max_size, None)
            }
            SizingMode::InherentSize => {
                let aspect_ratio = style.aspect_ratio();
                let style_size = style
                    .size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_apply_aspect_ratio(aspect_ratio)
                    .maybe_add(box_sizing_adjustment);
                let style_min_size = style
                    .min_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_apply_aspect_ratio(aspect_ratio)
                    .maybe_add(box_sizing_adjustment);
                let style_max_size = style
                    .max_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment);

                let node_size =
                    known_dimensions.or(style_size.maybe_clamp(style_min_size, style_max_size));
                (node_size, style_min_size, style_max_size, aspect_ratio)
            }
        };

        // Compute available space
        let available_space = Size {
            width: known_dimensions
                .width
                .map(AvailableSpace::from)
                .unwrap_or(available_space.width)
                .maybe_sub(margin.horizontal_axis_sum())
                .maybe_set(known_dimensions.width)
                .maybe_set(node_size.width)
                .map_definite_value(|size| {
                    size.maybe_clamp(node_min_size.width, node_max_size.width)
                        - content_box_inset.horizontal_axis_sum()
                }),
            height: known_dimensions
                .height
                .map(AvailableSpace::from)
                .unwrap_or(available_space.height)
                .maybe_sub(margin.vertical_axis_sum())
                .maybe_set(known_dimensions.height)
                .maybe_set(node_size.height)
                .map_definite_value(|size| {
                    size.maybe_clamp(node_min_size.height, node_max_size.height)
                        - content_box_inset.vertical_axis_sum()
                }),
        };

        // Compute size of inline boxes
        let child_inputs = taffy::tree::LayoutInput {
            known_dimensions: Size::NONE,
            available_space,
            sizing_mode: SizingMode::InherentSize,
            parent_size: available_space.into_options(),
            ..inputs
        };
        #[cfg(feature = "floats")]
        let float_child_inputs = taffy::tree::LayoutInput {
            available_space: Size::MAX_CONTENT,
            ..child_inputs
        };

        // Update inline boxes
        for ibox in inline_layout.layout.inline_boxes_mut() {
            let style = &self.nodes[ibox.id as usize].style;
            let margin = style
                .margin
                .resolve_or_zero(inputs.parent_size, resolve_calc_value);

            #[cfg(feature = "floats")]
            let is_floated = style.float.is_floated();
            #[cfg(not(feature = "floats"))]
            let is_floated = false;

            if style.position == Position::Absolute || is_floated {
                ibox.width = 0.0;
                ibox.height = 0.0;
            } else {
                let output = self.compute_child_layout(NodeId::from(ibox.id), child_inputs);
                ibox.baseline = output.first_baselines.y;
                ibox.width = (margin.left + margin.right + output.size.width) * scale;
                ibox.height = (margin.top + margin.bottom + output.size.height) * scale;
            }
        }

        // TODO: Resolve against style widths as well as known dimensions
        let text_indent = self.nodes[node_id]
            .primary_styles()
            .map(|s| s.clone_text_indent())
            .unwrap_or_else(GenericTextIndent::zero);
        let resolved_text_indent = text_indent
            .length
            .resolve(CSSPixelLength::new(known_dimensions.width.unwrap_or(0.0)))
            .px();
        inline_layout.layout.set_text_indent(
            resolved_text_indent,
            // NOTE: hanging and each_line don't current work because parsing them is cfg'd out in Stylo
            // due to Servo not yet supporting those features. They should start to "just work" in Blitz
            // once support is enabled in Stylo.
            IndentOptions {
                each_line: text_indent.each_line,
                hanging: text_indent.hanging,
            },
        );

        let pbw = container_pb.horizontal_components().sum() * scale;
        let width = known_dimensions
            .width
            .map(|w| (w * scale) - pbw)
            .unwrap_or_else(|| {
                // TODO: Cache content widths.
                //
                // This is a little tricky as the size of the inline boxes may depend on whether we are sizing under
                // and a min-content or max-content constraint. So if we want to compute both widths in one pass then
                // we need to store both a min-content and max-content size on each box.
                let content_sizes = inline_layout.layout.calculate_content_widths();
                let min_content_width = content_sizes.min;
                let max_content_width = content_sizes.max;

                #[cfg(feature = "floats")]
                let float_width = match available_space.width {
                    AvailableSpace::Definite(_) => 0.0,
                    AvailableSpace::MinContent => {
                        let mut width: f32 = 0.0;
                        for ibox in inline_layout.layout.inline_boxes_mut() {
                            let style = &self.nodes[ibox.id as usize].style;

                            if style.float.is_floated() {
                                let margin = style
                                    .margin
                                    .resolve_or_zero(inputs.parent_size, resolve_calc_value);
                                let output =
                                    self.compute_child_layout(NodeId::from(ibox.id), child_inputs);
                                width = width.max(output.size.width + margin.left + margin.right);
                            }
                        }

                        width * scale
                    }
                    AvailableSpace::MaxContent => {
                        let mut width: f32 = 0.0;
                        for ibox in inline_layout.layout.inline_boxes_mut() {
                            let style = &self.nodes[ibox.id as usize].style;

                            if style.float.is_floated() {
                                let margin = style
                                    .margin
                                    .resolve_or_zero(inputs.parent_size, resolve_calc_value);
                                let output =
                                    self.compute_child_layout(NodeId::from(ibox.id), child_inputs);
                                width += output.size.width + margin.left + margin.right;
                            }
                        }

                        width * scale
                    }
                };

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

        #[cfg(not(feature = "floats"))]
        let _ = block_ctx; // Suppress unused variable warning

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
        // block_ctx.apply_content_box_inset([container_pb.left, container_pb.right]);

        // if inputs.run_mode == taffy::RunMode::ComputeSize {
        //     // Height SHOULD be ignored if RequestedAxis is Horizontal, but currently that doesn't
        //     // always seem to be the case. So we perform layout to obtain a height every time. We
        //     // perform layout on a clone of the Layout to avoid clobbering the actual layout which
        //     // was causing https://github.com/DioxusLabs/blitz/pull/247#issuecomment-3235111617
        //     //
        //     // Doing this does seem to be as slow as one might expect, and if it enables correct
        //     // incremental layout then that is overall a big performance win.
        //     //
        //     // FIXME: avoid the need to clone the layout each time
        //     let mut layout = inline_layout.clone();
        //     layout.layout.break_all_lines(Some(width));

        //     return taffy::Size {
        //         width: width.ceil() / scale,
        //         height: layout.layout.height() / scale,
        //     };
        // }

        #[cfg(not(feature = "floats"))]
        {
            inline_layout.layout.break_all_lines(Some(width));
        }

        // Perform inline layout
        #[cfg(feature = "floats")]
        {
            let mut breaker = inline_layout.layout.break_lines();
            let initial_slot = block_ctx.find_content_slot(0.0, Clear::None, None);
            let mut has_active_floats = initial_slot.segment_id.is_some();
            let state = breaker.state_mut();
            state.set_layout_max_advance(width);
            state.set_line_max_advance(initial_slot.width * scale);
            state.set_line_x(initial_slot.x * scale);
            state.set_line_y((initial_slot.y * scale) as f64);

            // TODO: revert state and retry layout if a line doesn't fit
            //
            // Save initial state. Saved state is used to revert the layout to a previous state if needed
            // (e.g. to revert a line that doesn't fit in the space it was laid out into)
            //
            // let mut saved_state = breaker.state().clone();

            while let Some(yield_data) = breaker.break_next() {
                match yield_data {
                    YieldData::LineBreak(_line_break_data) => {
                        let state = breaker.state_mut();

                        if has_active_floats {
                            // TODO: revert state and retry layout if a line doesn't fit
                            // saved_state = state.clone();

                            let min_y = state.line_y() / scale as f64;
                            let next_slot =
                                block_ctx.find_content_slot(min_y as f32, Clear::None, None);
                            has_active_floats = next_slot.segment_id.is_some();

                            state.set_line_max_advance(next_slot.width * scale);
                            state.set_line_x(next_slot.x * scale);
                            state.set_line_y((next_slot.y * scale) as f64);
                        } else {
                            state.set_line_x(0.0);
                            state.set_line_max_advance(width);
                        }

                        continue;
                    }
                    YieldData::MaxHeightExceeded(_data) => {
                        // TODO
                        continue;
                    }
                    YieldData::InlineBoxBreak(box_break_data) => {
                        let state = breaker.state_mut();
                        let node_id = box_break_data.inline_box_id as usize;
                        let node = &mut self.nodes[node_id];

                        // We can assume that the box is a float because we only set `break_on_box: true` for floats
                        let direction = match node.style.float {
                            Float::Left => taffy::FloatDirection::Left,
                            Float::Right => taffy::FloatDirection::Right,
                            Float::None => unreachable!(),
                        };
                        let clear = node.style.clear;
                        let margin = node
                            .style
                            .margin
                            .resolve_or_zero(inputs.parent_size, resolve_calc_value);

                        let margin_sum = margin.sum_axes();

                        let output =
                            self.compute_child_layout(NodeId::from(node_id), float_child_inputs);
                        let min_y = state.line_y() as f32 / scale;
                        let mut pos = block_ctx.place_floated_box(
                            output.size + margin_sum,
                            min_y,
                            direction,
                            clear,
                        );
                        pos.x += container_pb.left;
                        pos.y += container_pb.top;

                        let min_y = state.line_y() / scale as f64; //.max(pos.y as f64);
                        let next_slot =
                            block_ctx.find_content_slot(min_y as f32, Clear::None, None);
                        has_active_floats = next_slot.segment_id.is_some();

                        state.set_line_max_advance(next_slot.width * scale);
                        state.set_line_x(next_slot.x * scale);
                        state.set_line_y((next_slot.y * scale) as f64);

                        let layout = &mut self.nodes[node_id].unrounded_layout;
                        layout.size = output.size;
                        layout.location.x = pos.x + margin.left + container_pb.left;
                        layout.location.y = pos.y + margin.top + container_pb.top;

                        // dbg!(&layout.size);
                        // dbg!(&layout.location);

                        state.append_inline_box_to_line(box_break_data.advance, 0.0, 0.0);

                        // if float.is_floated() {
                        //     println!("INLINE FLOATED BOX ({}) {:?}", ibox.id, float);
                        //     println!(
                        //         "w:{} h:{} x:{}, y:{}",
                        //         layout.size.width, layout.size.height, 0, 0
                        //     );
                        // }
                    }
                }
            }
            breaker.finish();
        }

        let alignment = self.nodes[node_id]
            .primary_styles()
            .map(|s| {
                use parley::layout::Alignment;
                use style::values::specified::TextAlignKeyword;

                match s.clone_text_align() {
                    TextAlignKeyword::Start => Alignment::Start,
                    TextAlignKeyword::Left => Alignment::Left,
                    TextAlignKeyword::Right => Alignment::Right,
                    TextAlignKeyword::Center => Alignment::Center,
                    TextAlignKeyword::Justify => Alignment::Justify,
                    TextAlignKeyword::End => Alignment::End,
                    TextAlignKeyword::MozCenter => Alignment::Center,
                    TextAlignKeyword::MozLeft => Alignment::Left,
                    TextAlignKeyword::MozRight => Alignment::Right,
                }
            })
            .unwrap_or(parley::layout::Alignment::Start);

        inline_layout.layout.align(
            alignment,
            AlignmentOptions {
                align_when_overflowing: false,
            },
        );

        #[allow(unused_mut)]
        let mut height = inline_layout.layout.height();

        #[cfg(feature = "floats")]
        {
            let contains_floats = is_bfc_root;
            if contains_floats {
                height = height.max(
                    (block_ctx.floated_content_height_contribution() + container_pb.top) * scale,
                )
            };
        }

        let measured_size = inputs.known_dimensions.unwrap_or(taffy::Size {
            width: width / scale,
            height: height / scale,
        });

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

        // Store sizes and positions of inline boxes
        for line in inline_layout.layout.lines() {
            for item in line.items() {
                if let parley::layout::PositionedLayoutItem::InlineBox(ibox) = item {
                    let node = &mut self.nodes[ibox.id as usize];
                    let padding = node
                        .style
                        .padding
                        .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);
                    let border = node
                        .style
                        .border
                        .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);
                    let margin = node
                        .style
                        .margin
                        .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);

                    #[cfg(feature = "floats")]
                    let is_floated = node.style.float != Float::None;
                    #[cfg(not(feature = "floats"))]
                    let is_floated = false;

                    if node.style.position == Position::Absolute {
                        let direction = node.style.direction;
                        layout_abspos_child(self, ibox, final_size, taffy::Point::ZERO, direction);
                    } else if is_floated {
                        let layout = &mut self.nodes[ibox.id as usize].unrounded_layout;
                        layout.padding = padding; //.map(|p| p / scale);
                        layout.border = border; //.map(|p| p / scale);
                    } else {
                        let layout = &mut node.unrounded_layout;
                        layout.size.width = (ibox.width / scale) - margin.left - margin.right;
                        layout.size.height = (ibox.height / scale) - margin.top - margin.bottom;
                        layout.location.x = (ibox.x / scale) + margin.left + container_pb.left;
                        layout.location.y = (ibox.y / scale) + margin.top + container_pb.top;
                        layout.padding = padding; //.map(|p| p / scale);
                        layout.border = border; //.map(|p| p / scale);
                    }
                }
            }
        }

        // println!("INLINE LAYOUT FOR {:?}. max_advance: {:?}", node_id, max_advance);
        // dbg!(&inline_layout.text);
        // println!("Computed: w: {} h: {}", inline_layout.layout.width(), inline_layout.layout.height());
        // println!("known_dimensions: w: {:?} h: {:?}", inputs.known_dimensions.width, inputs.known_dimensions.height);
        // println!("\n");

        let first_baseline = inline_layout
            .layout
            .first_baseline()
            .map(|baseline| baseline + content_box_inset.top);

        // Put layout back
        self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .inline_layout_data = Some(inline_layout);

        LayoutOutput {
            size: final_size,
            content_size: measured_size + padding.sum_axes(),
            first_baselines: Point {
                x: None,
                y: first_baseline,
            },
            top_margin: CollapsibleMarginSet::ZERO,
            bottom_margin: CollapsibleMarginSet::ZERO,
            margins_can_collapse_through: !has_styles_preventing_being_collapsed_through
                && final_size.height == 0.0
                && measured_size.height == 0.0,
        }
    }
}

#[inline(always)]
fn f32_max(a: f32, b: f32) -> f32 {
    a.max(b)
}

/// Perform absolute layout on all absolutely positioned children.
#[inline]
fn layout_abspos_child(
    tree: &mut impl taffy::LayoutBlockContainer,
    item: PositionedInlineBox,
    area_size: Size<f32>,
    area_offset: Point<f32>,
    direction: taffy::Direction,
) {
    let area_width = area_size.width;
    let area_height = area_size.height;

    let node_id = NodeId::new(item.id);
    let child_style = tree.get_block_child_style(node_id);

    // Skip items that are display:none or are not position:absolute
    if child_style.box_generation_mode() == taffy::BoxGenerationMode::None
        || child_style.position() != taffy::Position::Absolute
    {
        return;
    }

    let aspect_ratio = child_style.aspect_ratio();
    let overflow = child_style.overflow();
    let scrollbar_width = child_style.scrollbar_width();
    let margin = child_style
        .margin()
        .map(|margin| margin.resolve_to_option(area_width, resolve_calc_value));
    let padding = child_style
        .padding()
        .resolve_or_zero(Some(area_width), resolve_calc_value);
    let border = child_style
        .border()
        .resolve_or_zero(Some(area_width), resolve_calc_value);
    let padding_border_sum = (padding + border).sum_axes();
    let box_sizing_adjustment = if child_style.box_sizing() == taffy::BoxSizing::ContentBox {
        padding_border_sum
    } else {
        Size::ZERO
    };

    // Resolve inset
    let left = child_style
        .inset()
        .left
        .maybe_resolve(area_width, resolve_calc_value);
    let right = child_style
        .inset()
        .right
        .maybe_resolve(area_width, resolve_calc_value);
    let top = child_style
        .inset()
        .top
        .maybe_resolve(area_height, resolve_calc_value);
    let bottom = child_style
        .inset()
        .bottom
        .maybe_resolve(area_height, resolve_calc_value);

    // Compute known dimensions from min/max/inherent size styles
    let style_size = child_style
        .size()
        .maybe_resolve(area_size, resolve_calc_value)
        .maybe_apply_aspect_ratio(aspect_ratio)
        .maybe_add(box_sizing_adjustment);
    let min_size = child_style
        .min_size()
        .maybe_resolve(area_size, resolve_calc_value)
        .maybe_apply_aspect_ratio(aspect_ratio)
        .maybe_add(box_sizing_adjustment)
        .or(padding_border_sum.map(Some))
        .maybe_max(padding_border_sum);
    let max_size = child_style
        .max_size()
        .maybe_resolve(area_size, resolve_calc_value)
        .maybe_apply_aspect_ratio(aspect_ratio)
        .maybe_add(box_sizing_adjustment);
    let mut known_dimensions = style_size.maybe_clamp(min_size, max_size);

    dbg!(style_size);
    dbg!(area_size);

    drop(child_style);

    // Fill in width from left/right and reapply aspect ratio if:
    //   - Width is not already known
    //   - Item has both left and right inset properties set
    if let (None, Some(left), Some(right)) = (known_dimensions.width, left, right) {
        let new_width_raw =
            area_width.maybe_sub(margin.left).maybe_sub(margin.right) - left - right;
        known_dimensions.width = Some(f32_max(new_width_raw, 0.0));
        known_dimensions = known_dimensions
            .maybe_apply_aspect_ratio(aspect_ratio)
            .maybe_clamp(min_size, max_size);
    }

    // Fill in height from top/bottom and reapply aspect ratio if:
    //   - Height is not already known
    //   - Item has both top and bottom inset properties set
    if let (None, Some(top), Some(bottom)) = (known_dimensions.height, top, bottom) {
        let new_height_raw =
            area_height.maybe_sub(margin.top).maybe_sub(margin.bottom) - top - bottom;
        known_dimensions.height = Some(f32_max(new_height_raw, 0.0));
        known_dimensions = known_dimensions
            .maybe_apply_aspect_ratio(aspect_ratio)
            .maybe_clamp(min_size, max_size);
    }

    let measured_size = tree
        .compute_child_layout(
            node_id,
            taffy::LayoutInput {
                known_dimensions,
                parent_size: area_size.map(Some),
                available_space: Size {
                    width: AvailableSpace::Definite(
                        area_width.maybe_clamp(min_size.width, max_size.width),
                    ),
                    height: AvailableSpace::Definite(
                        area_height.maybe_clamp(min_size.height, max_size.height),
                    ),
                },
                sizing_mode: SizingMode::ContentSize,
                run_mode: RunMode::ComputeSize,
                axis: taffy::RequestedAxis::Both,
                vertical_margins_are_collapsible: taffy::Line::FALSE,
            },
        )
        .size;

    let final_size = known_dimensions
        .unwrap_or(measured_size)
        .maybe_clamp(min_size, max_size);

    let layout_output = tree.compute_child_layout(
        node_id,
        taffy::LayoutInput {
            known_dimensions: final_size.map(Some),
            parent_size: area_size.map(Some),
            available_space: Size {
                width: AvailableSpace::Definite(
                    area_width.maybe_clamp(min_size.width, max_size.width),
                ),
                height: AvailableSpace::Definite(
                    area_height.maybe_clamp(min_size.height, max_size.height),
                ),
            },
            sizing_mode: SizingMode::ContentSize,
            run_mode: RunMode::PerformLayout,
            axis: taffy::RequestedAxis::Both,
            vertical_margins_are_collapsible: taffy::Line::FALSE,
        },
    );

    let non_auto_margin = taffy::Rect {
        left: if left.is_some() {
            margin.left.unwrap_or(0.0)
        } else {
            0.0
        },
        right: if right.is_some() {
            margin.right.unwrap_or(0.0)
        } else {
            0.0
        },
        top: if top.is_some() {
            margin.top.unwrap_or(0.0)
        } else {
            0.0
        },
        bottom: if bottom.is_some() {
            margin.bottom.unwrap_or(0.0)
        } else {
            0.0
        },
    };

    // Expand auto margins to fill available space
    // https://www.w3.org/TR/CSS21/visudet.html#abs-non-replaced-width
    let auto_margin = {
        // Auto margins for absolutely positioned elements in block containers only resolve
        // if inset is set. Otherwise they resolve to 0.
        let absolute_auto_margin_space = Point {
            x: right
                .map(|right| area_size.width - right - left.unwrap_or(0.0))
                .unwrap_or(final_size.width),
            y: bottom
                .map(|bottom| area_size.height - bottom - top.unwrap_or(0.0))
                .unwrap_or(final_size.height),
        };
        let free_space = Size {
            width: absolute_auto_margin_space.x
                - final_size.width
                - non_auto_margin.horizontal_axis_sum(),
            height: absolute_auto_margin_space.y
                - final_size.height
                - non_auto_margin.vertical_axis_sum(),
        };

        let auto_margin_size = Size {
            // If all three of 'left', 'width', and 'right' are 'auto': First set any 'auto' values for 'margin-left' and 'margin-right' to 0.
            // Then, if the 'direction' property of the element establishing the static-position containing block is 'ltr' set 'left' to the
            // static position and apply rule number three below; otherwise, set 'right' to the static position and apply rule number one below.
            //
            // If none of the three is 'auto': If both 'margin-left' and 'margin-right' are 'auto', solve the equation under the extra constraint
            // that the two margins get equal values, unless this would make them negative, in which case when direction of the containing block is
            // 'ltr' ('rtl'), set 'margin-left' ('margin-right') to zero and solve for 'margin-right' ('margin-left'). If one of 'margin-left' or
            // 'margin-right' is 'auto', solve the equation for that value. If the values are over-constrained, ignore the value for 'left' (in case
            // the 'direction' property of the containing block is 'rtl') or 'right' (in case 'direction' is 'ltr') and solve for that value.
            width: {
                let auto_margin_count = margin.left.is_none() as u8 + margin.right.is_none() as u8;
                if auto_margin_count == 2
                    && (style_size.width.is_none() || style_size.width.unwrap() >= free_space.width)
                {
                    0.0
                } else if auto_margin_count > 0 {
                    free_space.width / auto_margin_count as f32
                } else {
                    0.0
                }
            },
            height: {
                let auto_margin_count = margin.top.is_none() as u8 + margin.bottom.is_none() as u8;
                if auto_margin_count == 2
                    && (style_size.height.is_none()
                        || style_size.height.unwrap() >= free_space.height)
                {
                    0.0
                } else if auto_margin_count > 0 {
                    free_space.height / auto_margin_count as f32
                } else {
                    0.0
                }
            },
        };

        taffy::Rect {
            left: margin.left.map(|_| 0.0).unwrap_or(auto_margin_size.width),
            right: margin.right.map(|_| 0.0).unwrap_or(auto_margin_size.width),
            top: margin.top.map(|_| 0.0).unwrap_or(auto_margin_size.height),
            bottom: margin
                .bottom
                .map(|_| 0.0)
                .unwrap_or(auto_margin_size.height),
        }
    };

    let resolved_margin = taffy::Rect {
        left: margin.left.unwrap_or(auto_margin.left),
        right: margin.right.unwrap_or(auto_margin.right),
        top: margin.top.unwrap_or(auto_margin.top),
        bottom: margin.bottom.unwrap_or(auto_margin.bottom),
    };

    let x_offset = match (left, right) {
        (Some(left), Some(right)) => {
            if direction == Direction::Rtl {
                area_size.width - final_size.width - right - resolved_margin.right
            } else {
                left + resolved_margin.left
            }
        }
        (Some(left), None) => left + resolved_margin.left,
        (None, Some(right)) => area_size.width - final_size.width - right - resolved_margin.right,
        (None, None) => {
            if direction == Direction::Rtl {
                item.x - final_size.width - resolved_margin.right - area_offset.x
            } else {
                item.x + resolved_margin.left - area_offset.x
            }
        }
    };
    let location = Point {
        x: x_offset + area_offset.x,
        y: top
            .map(|top| top + resolved_margin.top)
            .or(bottom.map(|bottom| {
                area_size.height - final_size.height - bottom - resolved_margin.bottom
            }))
            .maybe_add(area_offset.y)
            .unwrap_or(item.y + resolved_margin.top),
    };
    // Note: axis intentionally switched here as scrollbars take up space in the opposite axis
    // to the axis in which scrolling is enabled.
    let scrollbar_size = Size {
        width: if overflow.y == Overflow::Scroll {
            scrollbar_width
        } else {
            0.0
        },
        height: if overflow.x == Overflow::Scroll {
            scrollbar_width
        } else {
            0.0
        },
    };

    tree.set_unrounded_layout(
        node_id,
        &taffy::Layout {
            order: 0, // TODO: order
            size: final_size,
            content_size: layout_output.content_size,
            scrollbar_size,
            location,
            padding,
            border,
            margin: resolved_margin,
        },
    );
}
