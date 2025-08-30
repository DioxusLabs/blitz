use parley::AlignmentOptions;
use taffy::{
    AvailableSpace, LayoutPartialTree as _, MaybeMath as _, MaybeResolve as _, NodeId, Position,
    ResolveOrZero as _, Size, compute_leaf_layout,
};

use super::resolve_calc_value;
use crate::BaseDocument;

impl BaseDocument {
    pub(crate) fn compute_inline_layout(
        &mut self,
        node_id: usize,
        inputs: taffy::tree::LayoutInput,
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
            |_known_dimensions, available_space| {
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
                for ibox in inline_layout.layout.inline_boxes_mut() {
                    let style = &self.nodes[ibox.id as usize].style;
                    let margin = style
                        .margin
                        .resolve_or_zero(inputs.parent_size, resolve_calc_value);

                    if style.position == Position::Absolute {
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

                let width = inputs
                    .known_dimensions
                    .width
                    .map(|w| (w * scale) - pbw)
                    .unwrap_or_else(|| {
                        let content_sizes = inline_layout.content_widths();
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

                // Perform inline layout
                inline_layout.layout.break_all_lines(Some(width));

                if inputs.run_mode == taffy::RunMode::ComputeSize {
                    return taffy::Size {
                        width: width.ceil() / scale,
                        // Height will be ignored in RequestedAxis is Horizontal
                        height: inline_layout.layout.height() / scale,
                    };
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
                            TextAlignKeyword::Center => Alignment::Middle,
                            TextAlignKeyword::Justify => Alignment::Justified,
                            TextAlignKeyword::End => Alignment::End,
                            TextAlignKeyword::MozCenter => Alignment::Middle,
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
