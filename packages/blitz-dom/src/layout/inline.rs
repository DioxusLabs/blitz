use parley::fontique::{Attributes, QueryFont, QueryStatus};
use parley::{AlignmentOptions, IndentOptions};
use skrifa::MetadataProvider as _;
use style::values::{computed::CSSPixelLength, generics::text::GenericTextIndent};
use taffy::{
    AvailableSpace, BlockContext, BlockFormattingContext, BoxSizing, CollapsibleMarginSet,
    CoreStyle as _, LayoutInput, LayoutOutput, LayoutPartialTree as _, MaybeMath as _,
    MaybeResolve as _, NodeId, Overflow, Point, Position, ResolveOrZero as _, RunMode, Size,
    SizingMode,
};

#[cfg(feature = "floats")]
use taffy::{Clear, Float, prelude::TaffyMaxContent};

use super::construct::resolve_line_height;
use super::resolve_calc_value;
use crate::BaseDocument;
use crate::stylo_to_parley;

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

            // CSS2.1 §10.8.1: overflow != visible → baseline = bottom margin edge
            let overflow_not_visible = style.overflow.x.is_scroll_container()
                || style.overflow.y.is_scroll_container();

            if style.position == Position::Absolute || is_floated {
                ibox.width = 0.0;
                ibox.height = 0.0;
            } else {
                let output = self.compute_child_layout(NodeId::from(ibox.id), child_inputs);
                ibox.width = (margin.left + margin.right + output.size.width) * scale;
                ibox.height = (margin.top + margin.bottom + output.size.height) * scale;

                // CSS2.1 §10.8.1: the baseline of an inline-block is the baseline
                // of its last in-flow line box, unless it has no in-flow line boxes
                // or its overflow is not visible, in which case it's the bottom
                // margin edge (represented as first_baseline = None).
                //
                // Note: the internal baseline may exceed the box height when the
                // box has an explicit small height but inherits a large line-height.
                // This is correct — parley's metric clamping (shift_offset==0 → clamp)
                // prevents line expansion while the full baseline is used for positioning.
                ibox.first_baseline = if overflow_not_visible {
                    None
                } else {
                    output.first_baselines.y.map(|b| (b + margin.top) * scale)
                };
            }

            // Re-read baseline_shift from node style each time to avoid accumulating
            // scales across repeated layout passes (e.g. flex sizing calls this multiple times)
            let node_for_shift = &self.nodes[ibox.id as usize];
            if let Some(styles) = node_for_shift.primary_styles() {
                let bs = crate::stylo_to_parley::baseline_shift(&styles);
                ibox.baseline_shift = match bs {
                    parley::style::BaselineShift::Length(v) => {
                        parley::style::BaselineShift::Length(v * scale)
                    }
                    other => other,
                };
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

        // CSS strut (CSS2.1 §10.8.1): set minimum line metrics from root font
        if let Some((strut_ascent, strut_descent, strut_line_height, strut_x_height)) =
            self.compute_strut_metrics(node_id, scale)
        {
            inline_layout.layout.set_strut(
                strut_ascent,
                strut_descent,
                strut_line_height,
                strut_x_height,
            );
        }

        #[cfg(not(feature = "floats"))]
        {
            inline_layout.layout.break_all_lines(Some(width));
        }

        // Perform inline layout
        // TODO: Re-implement float-aware line breaking with new parley BreakLines API.
        // The new API no longer exposes state_mut()/YieldData, so float interaction
        // during line breaking is not yet supported. Float positioning is handled
        // in the post-layout loop below.
        #[cfg(feature = "floats")]
        {
            inline_layout.layout.break_all_lines(Some(width));
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
            Some(width),
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

        let final_size = inputs.known_dimensions.unwrap_or(taffy::Size {
            width: width / scale,
            height: height / scale,
        });

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

                    // Resolve inset
                    let left = node
                        .style
                        .inset
                        .left
                        .maybe_resolve(final_size.width, resolve_calc_value);
                    let right = node
                        .style
                        .inset
                        .right
                        .maybe_resolve(final_size.width, resolve_calc_value);
                    let top = node
                        .style
                        .inset
                        .top
                        .maybe_resolve(final_size.height, resolve_calc_value);
                    let bottom = node
                        .style
                        .inset
                        .bottom
                        .maybe_resolve(final_size.height, resolve_calc_value);

                    #[cfg(feature = "floats")]
                    let is_floated = node.style.float != Float::None;
                    #[cfg(not(feature = "floats"))]
                    let is_floated = false;

                    if node.style.position == Position::Absolute {
                        let output = self.compute_child_layout(NodeId::from(ibox.id), child_inputs);

                        let layout = &mut self.nodes[ibox.id as usize].unrounded_layout;
                        layout.size = output.size;

                        // TODO: Implement absolute positioning
                        layout.location.x = left
                            .map(|left| left + margin.left)
                            .or_else(|| {
                                right.map(|right| {
                                    final_size.width - right - output.size.width - margin.right
                                })
                            })
                            .unwrap_or((ibox.x / scale) + margin.left + container_pb.left);
                        layout.location.y = top
                            .map(|top| top + margin.top)
                            .or_else(|| {
                                bottom.map(|bottom| {
                                    final_size.height - bottom - output.size.height - margin.bottom
                                })
                            })
                            .unwrap_or((ibox.y / scale) + margin.top + container_pb.top);

                        layout.padding = padding; //.map(|p| p / scale);
                        layout.border = border; //.map(|p| p / scale);
                    } else if is_floated {
                        #[cfg(feature = "floats")]
                        {
                            let float_dir = match self.nodes[ibox.id as usize].style.float {
                                Float::Left => taffy::FloatDirection::Left,
                                Float::Right => taffy::FloatDirection::Right,
                                Float::None => unreachable!(),
                            };
                            let clear = self.nodes[ibox.id as usize].style.clear;
                            let output = self
                                .compute_child_layout(NodeId::from(ibox.id), float_child_inputs);
                            let min_y = ibox.y / scale;
                            let pos = block_ctx.place_floated_box(
                                output.size + margin.sum_axes(),
                                min_y,
                                float_dir,
                                clear,
                            );
                            let layout = &mut self.nodes[ibox.id as usize].unrounded_layout;
                            layout.size = output.size;
                            layout.location.x = pos.x + margin.left + container_pb.left;
                            layout.location.y = pos.y + margin.top + container_pb.top;
                            layout.padding = padding;
                            layout.border = border;
                        }
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

        // CSS2.1 §10.8.1: the baseline of an inline-block is the baseline
        // of its last in-flow line box.
        //
        // When the box has an explicit height that clips later lines (e.g. height: 4px
        // but the content spans multiple lines), the last line's baseline may be outside
        // the box. In that case, walk backwards to find the last line whose baseline
        // fits within the box, falling back to the first line if none fit (handles the
        // case where even line 1's baseline exceeds a tiny explicit height).
        let content_height = final_size.height * scale;
        let last_baseline_y = inline_layout
            .layout
            .lines()
            .rev()
            .find(|line| line.metrics().baseline <= content_height)
            .or_else(|| inline_layout.layout.lines().next())
            .map(|line| (line.metrics().baseline / scale) + container_pb.top);

        // Put layout back
        self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .inline_layout_data = Some(inline_layout);

        let measured_size = final_size;

        let clamped_size = inputs
            .known_dimensions
            .or(node_size)
            .unwrap_or(measured_size + content_box_inset.sum_axes())
            .maybe_clamp(node_min_size, node_max_size);
        let size = Size {
            width: clamped_size.width,
            height: f32_max(
                clamped_size.height,
                aspect_ratio
                    .map(|ratio| clamped_size.width / ratio)
                    .unwrap_or(0.0),
            ),
        };
        let size = size.maybe_max(container_pb.sum_axes().map(Some));

        LayoutOutput {
            size,
            content_size: measured_size + padding.sum_axes(),
            first_baselines: Point {
                x: None,
                y: last_baseline_y,
            },
            top_margin: CollapsibleMarginSet::ZERO,
            bottom_margin: CollapsibleMarginSet::ZERO,
            margins_can_collapse_through: !has_styles_preventing_being_collapsed_through
                && size.height == 0.0
                && measured_size.height == 0.0,
        }
    }

    /// Compute CSS strut metrics (CSS2.1 §10.8.1) for an inline formatting context root.
    ///
    /// Returns `(ascent, descent, line_height, x_height)` in physical pixels, or `None`
    /// if the root node's font cannot be resolved.
    fn compute_strut_metrics(&self, node_id: usize, scale: f32) -> Option<(f32, f32, f32, f32)> {
        let styles = self.nodes[node_id].primary_styles()?;
        let font_styles = styles.get_font();

        // Resolve font size and line height in CSS pixels
        let font_size_px = font_styles.font_size.used_size.0.px();
        let line_height = match font_styles.line_height {
            stylo_to_parley::stylo::LineHeight::Normal => parley::LineHeight::FontSizeRelative(1.2),
            stylo_to_parley::stylo::LineHeight::Number(n) => {
                parley::LineHeight::FontSizeRelative(n.0)
            }
            stylo_to_parley::stylo::LineHeight::Length(v) => parley::LineHeight::Absolute(v.0.px()),
        };

        // Query fontique for matching font
        let mut font_ctx = self.font_ctx.lock().unwrap();
        let font_ctx = &mut *font_ctx;
        let mut query = font_ctx.collection.query(&mut font_ctx.source_cache);

        let families = font_styles
            .font_family
            .families
            .iter()
            .map(stylo_to_parley::query_font_family);
        query.set_families(families);
        query.set_attributes(Attributes {
            width: stylo_to_parley::font_width(font_styles.font_stretch),
            weight: stylo_to_parley::font_weight(font_styles.font_weight),
            style: stylo_to_parley::font_style(font_styles.font_style),
        });

        // Find a font that supports the space character
        let mut font: Option<QueryFont> = None;
        query.matches_with(|q_font: &QueryFont| {
            let Ok(font_ref) = skrifa::FontRef::from_index(q_font.blob.as_ref(), q_font.index)
            else {
                return QueryStatus::Continue;
            };
            if font_ref.charmap().map(' ').is_some() {
                font = Some(q_font.clone());
                QueryStatus::Stop
            } else {
                QueryStatus::Continue
            }
        });
        let font = font?;

        // Get skrifa metrics at CSS-pixel font size
        let font_ref = skrifa::FontRef::from_index(font.blob.as_ref(), font.index).ok()?;
        let metrics = skrifa::metrics::Metrics::new(
            &font_ref,
            skrifa::instance::Size::new(font_size_px),
            skrifa::instance::LocationRef::default(),
        );

        // Scale to physical pixels
        let strut_ascent = metrics.ascent * scale;
        let strut_descent = -metrics.descent * scale; // skrifa descent is negative
        let strut_line_height = resolve_line_height(line_height, font_size_px) * scale;
        // x_height for vertical-align: middle; CSS spec fallback is font_size * 0.5
        let strut_x_height = metrics.x_height.unwrap_or(font_size_px * 0.5) * scale;

        Some((
            strut_ascent,
            strut_descent,
            strut_line_height,
            strut_x_height,
        ))
    }
}

#[inline(always)]
fn f32_max(a: f32, b: f32) -> f32 {
    a.max(b)
}
