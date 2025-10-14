use parley::{AlignmentOptions, BreakerState, YieldData};
use taffy::{
    AvailableSpace, BlockContext, BlockFormattingContext, Clear, Float, LayoutPartialTree as _,
    MaybeMath as _, MaybeResolve as _, NodeId, Position, ResolveOrZero as _, Size,
    compute_leaf_layout, prelude::TaffyMaxContent,
};

use super::resolve_calc_value;
use crate::BaseDocument;

impl BaseDocument {
    pub(crate) fn compute_inline_layout(
        &mut self,
        node_id: usize,
        inputs: taffy::tree::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::LayoutOutput {
        // Unwrap the block formatting context if one was passed, or else create a new one
        match block_ctx {
            Some(inherited_bfc) => self.compute_inline_layout_inner(node_id, inputs, inherited_bfc),
            None => {
                let mut root_bfc = BlockFormattingContext::new(inputs.available_space.width);
                let mut root_ctx = root_bfc.root_block_context();
                self.compute_inline_layout_inner(node_id, inputs, &mut root_ctx)
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

        // Take inline layout to satisfy borrow checker
        let mut inline_layout = self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .take_inline_layout()
            .unwrap();

        // TODO: eliminate clone
        let style = self.nodes[node_id].style.clone();

        let output = compute_leaf_layout(
            inputs,
            &style,
            resolve_calc_value,
            |known_dimensions, available_space| {
                // Short circuit if inline context contains no text or inline boxes
                if inline_layout.text.is_empty() && inline_layout.layout.inline_boxes().is_empty() {
                    return Size::ZERO;
                }

                // Compute size of inline boxes
                let child_inputs = taffy::tree::LayoutInput {
                    known_dimensions: Size::NONE,
                    available_space,
                    parent_size: available_space.into_options(),
                    ..inputs
                };
                let float_child_inputs = taffy::tree::LayoutInput {
                    available_space: Size::MAX_CONTENT,
                    ..child_inputs
                };
                for ibox in inline_layout.layout.inline_boxes_mut() {
                    let style = &self.nodes[ibox.id as usize].style;
                    let margin = style
                        .margin
                        .resolve_or_zero(inputs.parent_size, resolve_calc_value);

                    ibox.break_on_box = style.float.is_floated();

                    if style.position == Position::Absolute {
                        ibox.width = 0.0;
                        ibox.height = 0.0;
                    } else if style.float.is_floated() {
                        ibox.width = 0.0;
                        ibox.height = 0.0;
                    } else {
                        let output = self.compute_child_layout(NodeId::from(ibox.id), child_inputs);
                        ibox.width = (margin.left + margin.right + output.size.width) * scale;
                        ibox.height = (margin.top + margin.bottom + output.size.height) * scale;
                    }
                }

                // Determine width
                let padding = style
                    .padding
                    .resolve_or_zero(inputs.parent_size, resolve_calc_value);
                let border = style
                    .border
                    .resolve_or_zero(inputs.parent_size, resolve_calc_value);
                let container_pb = padding + border;
                let pbw = container_pb.horizontal_components().sum() * scale;

                // Create sub-context to account for the inline layout's padding/border
                let mut block_ctx = block_ctx
                    .sub_context(container_pb.top, [container_pb.left, container_pb.right]);

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
                        let computed_width = match available_space.width {
                            AvailableSpace::MinContent => content_sizes.min,
                            AvailableSpace::MaxContent => content_sizes.max,
                            AvailableSpace::Definite(limit) => (limit * scale)
                                .min(content_sizes.max)
                                .max(content_sizes.min),
                        }
                        .ceil();
                        let style_width = style
                            .size
                            .width
                            .maybe_resolve(inputs.parent_size.width, resolve_calc_value)
                            .map(|w| w * scale);
                        let min_width = style
                            .min_size
                            .width
                            .maybe_resolve(inputs.parent_size.width, resolve_calc_value)
                            .map(|w| w * scale);
                        let max_width = style
                            .max_size
                            .width
                            .maybe_resolve(inputs.parent_size.width, resolve_calc_value)
                            .map(|w| w * scale);

                        (style_width)
                            .unwrap_or(computed_width + pbw)
                            .max(computed_width)
                            .maybe_clamp(min_width, max_width)
                            - pbw
                    });

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

                // Perform inline layout
                let mut breaker = inline_layout.layout.break_lines();
                let mut line_width = width;
                let initial_slot = block_ctx.find_content_slot(0.0, Clear::None, None);
                let state = breaker.state_mut();
                state.set_layout_max_advance(width);
                state.set_line_max_advance(initial_slot.width * scale);
                state.set_line_x(initial_slot.x * scale);
                state.set_line_y((initial_slot.y * scale) as f64);

                // Save initial state. Saved state is used to revert the layout to a previous state if needed
                // (e.g. to revert a line that doesn't fit in the space it was laid out into)
                let mut saved_state: BreakerState = breaker.state().clone();

                while let Some(yield_data) = breaker.break_next() {
                    match yield_data {
                        YieldData::LineBreak(line_break_data) => {
                            let state = breaker.state_mut();
                            saved_state = state.clone();

                            let min_y = (state.line_y() + line_break_data.line_height as f64)
                                / scale as f64;
                            let next_slot =
                                block_ctx.find_content_slot(min_y as f32, Clear::None, None);

                            state.set_line_max_advance(next_slot.width * scale);
                            state.set_line_x(next_slot.x * scale);
                            state.set_line_y((next_slot.y * scale) as f64);

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
                            let output = self
                                .compute_child_layout(NodeId::from(node_id), float_child_inputs);
                            let min_y = (state.line_y() as f32 / scale) + container_pb.top;
                            let pos =
                                block_ctx.place_floated_box(output.size, min_y, direction, clear);

                            let min_y = (state.line_y() / scale as f64); //.max(pos.y as f64);
                            let next_slot =
                                block_ctx.find_content_slot(min_y as f32, Clear::None, None);

                            state.set_line_max_advance(next_slot.width * scale);
                            state.set_line_x(next_slot.x * scale);
                            state.set_line_y((next_slot.y * scale) as f64);

                            let layout = &mut self.nodes[node_id].unrounded_layout;
                            layout.size = output.size;
                            layout.location.x = pos.x;
                            layout.location.y = pos.y;

                            // dbg!(&layout.size);
                            // dbg!(&layout.location);

                            state.append_inline_box_to_line(box_break_data.advance);

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
                                .maybe_resolve(child_inputs.parent_size.width, resolve_calc_value);
                            let right = node
                                .style
                                .inset
                                .right
                                .maybe_resolve(child_inputs.parent_size.width, resolve_calc_value);
                            let top = node
                                .style
                                .inset
                                .top
                                .maybe_resolve(child_inputs.parent_size.height, resolve_calc_value);
                            let bottom = node
                                .style
                                .inset
                                .bottom
                                .maybe_resolve(child_inputs.parent_size.height, resolve_calc_value);

                            let float = node.style.float;
                            if node.style.position == Position::Absolute {
                                let output =
                                    self.compute_child_layout(NodeId::from(ibox.id), child_inputs);

                                let layout = &mut self.nodes[ibox.id as usize].unrounded_layout;
                                layout.size = output.size;

                                // TODO: Implement absolute positioning
                                layout.location.x = left
                                    .or_else(|| {
                                        child_inputs
                                            .parent_size
                                            .width
                                            .zip(right)
                                            .map(|(w, r)| w - r)
                                    })
                                    .unwrap_or((ibox.x / scale) + margin.left + container_pb.left);
                                layout.location.y = top
                                    .or_else(|| {
                                        child_inputs
                                            .parent_size
                                            .height
                                            .zip(bottom)
                                            .map(|(w, r)| w - r)
                                    })
                                    .unwrap_or((ibox.y / scale) + margin.top + container_pb.top);

                                layout.padding = padding; //.map(|p| p / scale);
                                layout.border = border; //.map(|p| p / scale);
                            } else if float != Float::None {
                                let layout = &mut self.nodes[ibox.id as usize].unrounded_layout;
                                layout.padding = padding; //.map(|p| p / scale);
                                layout.border = border; //.map(|p| p / scale);
                            } else {
                                let layout = &mut node.unrounded_layout;
                                layout.size.width =
                                    (ibox.width / scale) - margin.left - margin.right;
                                layout.size.height =
                                    (ibox.height / scale) - margin.top - margin.bottom;
                                layout.location.x =
                                    (ibox.x / scale) + margin.left + container_pb.left;
                                layout.location.y =
                                    (ibox.y / scale) + margin.top + container_pb.top;
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

                inputs.known_dimensions.unwrap_or(taffy::Size {
                    width: inline_layout.layout.width().ceil() / scale,
                    height: inline_layout.layout.height() / scale,
                })
            },
        );

        // Put layout back
        self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .inline_layout_data = Some(inline_layout);

        output
    }
}
