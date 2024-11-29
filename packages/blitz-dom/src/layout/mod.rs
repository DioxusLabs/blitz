//! Enable the dom to lay itself out using taffy
//!
//! In servo, style and layout happen together during traversal
//! However, in Blitz, we do a style pass then a layout pass.
//! This is slower, yes, but happens fast enough that it's not a huge issue.

use crate::node::{ImageData, NodeData, NodeSpecificData};
use crate::{
    document::Document,
    image::{image_measure_function, ImageContext},
    node::Node,
};
use markup5ever::local_name;
use std::cell::Ref;
use std::sync::Arc;
use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_leaf_layout, prelude::*, Cache, FlexDirection, LayoutPartialTree, MaybeMath as _,
    MaybeResolve, NodeId, ResolveOrZero, RoundTree, Size, Style, TraversePartialTree, TraverseTree,
};

pub(crate) mod construct;
pub(crate) mod table;

use self::table::TableTreeWrapper;

impl Document {
    fn node_from_id(&self, node_id: taffy::prelude::NodeId) -> &Node {
        &self.nodes[node_id.into()]
    }
    fn node_from_id_mut(&mut self, node_id: taffy::prelude::NodeId) -> &mut Node {
        &mut self.nodes[node_id.into()]
    }
}

impl TraversePartialTree for Document {
    type ChildIter<'a> = RefCellChildIter<'a>;

    fn child_ids(&self, node_id: NodeId) -> Self::ChildIter<'_> {
        let layout_children = self.node_from_id(node_id).layout_children.borrow(); //.unwrap().as_ref();
        RefCellChildIter::new(Ref::map(layout_children, |children| {
            children.as_ref().map(|c| c.as_slice()).unwrap_or(&[])
        }))
    }

    fn child_count(&self, node_id: NodeId) -> usize {
        self.node_from_id(node_id)
            .layout_children
            .borrow()
            .as_ref()
            .map(|c| c.len())
            .unwrap_or(0)
    }

    fn get_child_id(&self, node_id: NodeId, index: usize) -> NodeId {
        NodeId::from(
            self.node_from_id(node_id)
                .layout_children
                .borrow()
                .as_ref()
                .unwrap()[index],
        )
    }
}
impl TraverseTree for Document {}

impl LayoutPartialTree for Document {
    type CoreContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;
    type CacheMut<'b>
        = &'b mut Cache
    where
        Self: 'b;

    fn get_core_container_style(&self, node_id: NodeId) -> &Style {
        &self.node_from_id(node_id).style
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.node_from_id_mut(node_id).unrounded_layout = *layout;
    }

    fn get_cache_mut(&mut self, node_id: NodeId) -> &mut Cache {
        &mut self.node_from_id_mut(node_id).cache
    }

    fn compute_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
    ) -> taffy::tree::LayoutOutput {
        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            let node = &mut tree.nodes[node_id.into()];

            let resolved_line_height = node.primary_styles().map(|style| {
                use style::values::computed::font::LineHeight;

                let font_size = style.clone_font_size().used_size().px();
                match style.clone_line_height() {
                    LineHeight::Normal => font_size * 1.2,
                    LineHeight::Number(num) => font_size * num.0,
                    LineHeight::Length(value) => value.0.px(),
                }
            });

            match &mut node.raw_dom_data {
                NodeData::Text(data) => {
                    // With the new "inline context" architecture all text nodes should be wrapped in an "inline layout context"
                    // and should therefore never be measured individually.
                    println!(
                        "ERROR: Tried to lay out text node individually ({})",
                        usize::from(node_id)
                    );
                    dbg!(data);
                    taffy::LayoutOutput::HIDDEN
                    // unreachable!();

                    // compute_leaf_layout(inputs, &node.style, |known_dimensions, available_space| {
                    //     let context = TextContext {
                    //         text_content: &data.content.trim(),
                    //         writing_mode: WritingMode::Horizontal,
                    //     };
                    //     let font_metrics = FontMetrics {
                    //         char_width: 8.0,
                    //         char_height: 16.0,
                    //     };
                    //     text_measure_function(
                    //         known_dimensions,
                    //         available_space,
                    //         &context,
                    //         &font_metrics,
                    //     )
                    // })
                }
                NodeData::Element(element_data) | NodeData::AnonymousBlock(element_data) => {
                    // Hide hidden nodes
                    if let Some("hidden" | "") = element_data.attr(local_name!("hidden")) {
                        node.style.display = Display::None;
                        return taffy::LayoutOutput::HIDDEN;
                    }

                    // todo: need to handle shadow roots by actually descending into them
                    if *element_data.name.local == *"input" {
                        match element_data.attr(local_name!("type")) {
                            // if the input type is hidden, hide it
                            Some("hidden") => {
                                node.style.display = Display::None;
                                return taffy::LayoutOutput::HIDDEN;
                            }
                            Some("checkbox") => {
                                return compute_leaf_layout(
                                    inputs,
                                    &node.style,
                                    |_known_size, _available_space| {
                                        let width = node
                                            .style
                                            .size
                                            .width
                                            .resolve_or_zero(inputs.parent_size.width);
                                        let height = node
                                            .style
                                            .size
                                            .height
                                            .resolve_or_zero(inputs.parent_size.height);
                                        let min_size = width.min(height);
                                        taffy::Size {
                                            width: min_size,
                                            height: min_size,
                                        }
                                    },
                                );
                            }
                            None | Some("text" | "password" | "email") => {
                                return compute_leaf_layout(
                                    inputs,
                                    &node.style,
                                    |_known_size, _available_space| taffy::Size {
                                        width: 300.0,
                                        height: resolved_line_height.unwrap_or(16.0),
                                    },
                                );
                            }
                            _ => {}
                        }
                    }

                    if *element_data.name.local == *"img" || *element_data.name.local == *"svg" {
                        // Get width and height attributes on image element
                        //
                        // TODO: smarter sizing using these (depending on object-fit, they shouldn't
                        // necessarily just override the native size)
                        let attr_size = taffy::Size {
                            width: element_data
                                .attr(local_name!("width"))
                                .and_then(|val| val.parse::<f32>().ok()),
                            height: element_data
                                .attr(local_name!("height"))
                                .and_then(|val| val.parse::<f32>().ok()),
                        };

                        // Get image's native size
                        let inherent_size = match &element_data.node_specific_data {
                            NodeSpecificData::Image(ImageData::Raster(data)) => taffy::Size {
                                width: data.image.width() as f32,
                                height: data.image.height() as f32,
                            },
                            NodeSpecificData::Image(ImageData::Svg(svg)) => {
                                let size = svg.size();
                                taffy::Size {
                                    width: size.width(),
                                    height: size.height(),
                                }
                            }
                            NodeSpecificData::None => taffy::Size {
                                width: 0.0,
                                height: 0.0,
                            },
                            _ => unreachable!(),
                        };

                        let image_context = ImageContext {
                            inherent_size,
                            attr_size,
                        };

                        let computed = compute_leaf_layout(
                            inputs,
                            &node.style,
                            |known_dimensions, _available_space| {
                                image_measure_function(
                                    known_dimensions,
                                    inputs.parent_size,
                                    &image_context,
                                    &node.style,
                                    false,
                                )
                            },
                        );

                        return computed;
                    }

                    if node.is_table_root {
                        let NodeSpecificData::TableRoot(context) = &tree.nodes[node_id.into()]
                            .raw_dom_data
                            .downcast_element()
                            .unwrap()
                            .node_specific_data
                        else {
                            panic!("Node marked as table root but doesn't have TableContext");
                        };
                        let context = Arc::clone(context);

                        let mut table_wrapper = TableTreeWrapper {
                            doc: tree,
                            ctx: context,
                        };
                        return compute_grid_layout(&mut table_wrapper, node_id, inputs);
                    }

                    if node.is_inline_root {
                        return tree.compute_inline_layout(node_id, inputs);
                    }

                    // The default CSS file will set
                    match node.style.display {
                        Display::Block => compute_block_layout(tree, node_id, inputs),
                        Display::Flex => compute_flexbox_layout(tree, node_id, inputs),
                        Display::Grid => compute_grid_layout(tree, node_id, inputs),
                        Display::None => taffy::LayoutOutput::HIDDEN,
                    }
                }
                NodeData::Document => compute_block_layout(tree, node_id, inputs),

                _ => taffy::LayoutOutput::HIDDEN,
            }
        })
    }
}

impl taffy::LayoutBlockContainer for Document {
    type BlockContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;

    type BlockItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_block_container_style(&self, node_id: NodeId) -> Self::BlockContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_block_child_style(&self, child_node_id: NodeId) -> Self::BlockItemStyle<'_> {
        self.get_core_container_style(child_node_id)
    }
}

impl taffy::LayoutFlexboxContainer for Document {
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;

    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node_id: NodeId) -> Self::FlexboxContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_flexbox_child_style(&self, child_node_id: NodeId) -> Self::FlexboxItemStyle<'_> {
        self.get_core_container_style(child_node_id)
    }
}

impl taffy::LayoutGridContainer for Document {
    type GridContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;

    type GridItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: NodeId) -> Self::GridItemStyle<'_> {
        self.get_core_container_style(child_node_id)
    }
}

impl Document {
    fn compute_inline_layout(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
    ) -> taffy::LayoutOutput {
        let scale = self.viewport.scale();

        // Take inline layout to satisfy borrow checker
        let mut inline_layout = self.nodes[usize::from(node_id)]
            .raw_dom_data
            .downcast_element_mut()
            .unwrap()
            .take_inline_layout()
            .unwrap();

        // TODO: eliminate clone
        let style = self.nodes[usize::from(node_id)].style.clone();

        let output = compute_leaf_layout(inputs, &style, |_known_dimensions, available_space| {
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
                let margin = style.margin.resolve_or_zero(inputs.parent_size);

                if style.position == Position::Absolute {
                    ibox.width = 0.0;
                    ibox.height = 0.0;
                } else {
                    let output = self.compute_child_layout(NodeId::from(ibox.id), child_inputs);
                    ibox.width = (margin.left + margin.right + output.size.width) * scale;
                    ibox.height = (margin.top + margin.bottom + output.size.height) * scale;
                }
            }

            // Perform inline layout
            let max_advance = match available_space.width {
                AvailableSpace::Definite(px) => Some(px * scale),
                AvailableSpace::MinContent => Some(0.0),
                AvailableSpace::MaxContent => None,
            };

            let alignment = self.nodes[usize::from(node_id)]
                .primary_styles()
                .map(|s| {
                    use parley::layout::Alignment;
                    use style::values::specified::TextAlignKeyword;

                    match s.clone_text_align() {
                        TextAlignKeyword::Start => Alignment::Start,
                        TextAlignKeyword::Left => Alignment::Start,
                        TextAlignKeyword::Right => Alignment::End,
                        TextAlignKeyword::Center => Alignment::Middle,
                        TextAlignKeyword::Justify => Alignment::Justified,
                        TextAlignKeyword::End => Alignment::End,
                        TextAlignKeyword::MozCenter => Alignment::Middle,
                        TextAlignKeyword::MozLeft => Alignment::Start,
                        TextAlignKeyword::MozRight => Alignment::End,
                    }
                })
                .unwrap_or(parley::layout::Alignment::Start);

            inline_layout.layout.break_all_lines(max_advance);

            let padding = style
                .padding
                .resolve_or_zero(inputs.parent_size)
                .map(|w| w * scale);
            let border = style
                .border
                .resolve_or_zero(inputs.parent_size)
                .map(|w| w * scale);

            let pbw = (padding + border).horizontal_components().sum() * scale;

            // Align layout
            let alignment_width = inputs
                .known_dimensions
                .width
                .map(|w| (w * scale) - pbw)
                .unwrap_or_else(|| {
                    let computed_width = inline_layout.layout.width();
                    let style_width = style
                        .size
                        .width
                        .maybe_resolve(inputs.parent_size.width)
                        .map(|w| w * scale);
                    let min_width = style
                        .min_size
                        .width
                        .maybe_resolve(inputs.parent_size.width)
                        .map(|w| w * scale);
                    let max_width = style
                        .max_size
                        .width
                        .maybe_resolve(inputs.parent_size.width)
                        .map(|w| w * scale);

                    (style_width)
                        .unwrap_or(computed_width + pbw)
                        .max(computed_width)
                        .maybe_clamp(min_width, max_width)
                        - pbw
                });

            inline_layout.layout.align(Some(alignment_width), alignment);

            // Store sizes and positions of inline boxes
            for line in inline_layout.layout.lines() {
                for item in line.items() {
                    if let parley::layout::PositionedLayoutItem::InlineBox(ibox) = item {
                        let node = &mut self.nodes[ibox.id as usize];
                        let padding = node.style.padding.resolve_or_zero(child_inputs.parent_size);
                        let border = node.style.border.resolve_or_zero(child_inputs.parent_size);
                        let margin = node.style.margin.resolve_or_zero(child_inputs.parent_size);

                        // Resolve inset
                        let left = node
                            .style
                            .inset
                            .left
                            .maybe_resolve(child_inputs.parent_size.width);
                        let right = node
                            .style
                            .inset
                            .right
                            .maybe_resolve(child_inputs.parent_size.width);
                        let top = node
                            .style
                            .inset
                            .top
                            .maybe_resolve(child_inputs.parent_size.height);
                        let bottom = node
                            .style
                            .inset
                            .bottom
                            .maybe_resolve(child_inputs.parent_size.height);

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
                                .unwrap_or(0.0);
                            layout.location.y = top
                                .or_else(|| {
                                    child_inputs
                                        .parent_size
                                        .height
                                        .zip(bottom)
                                        .map(|(w, r)| w - r)
                                })
                                .unwrap_or(0.0);

                            layout.padding = padding; //.map(|p| p / scale);
                            layout.border = border; //.map(|p| p / scale);
                        } else {
                            let layout = &mut node.unrounded_layout;
                            layout.size.width = (ibox.width / scale) - margin.left - margin.right;
                            layout.size.height = (ibox.height / scale) - margin.top - margin.bottom;
                            layout.location.x = (ibox.x / scale) + margin.left;
                            layout.location.y = (ibox.y / scale) + margin.top;
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
                width: inline_layout.layout.width() / scale,
                height: inline_layout.layout.height() / scale,
            })
        });

        // Put layout back
        self.nodes[usize::from(node_id)]
            .raw_dom_data
            .downcast_element_mut()
            .unwrap()
            .inline_layout_data = Some(inline_layout);

        output
    }
}

impl RoundTree for Document {
    fn get_unrounded_layout(&self, node_id: NodeId) -> &Layout {
        &self.node_from_id(node_id).unrounded_layout
    }

    fn set_final_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.node_from_id_mut(node_id).final_layout = *layout;
    }
}

impl PrintTree for Document {
    fn get_debug_label(&self, node_id: NodeId) -> &'static str {
        let node = &self.node_from_id(node_id);
        let style = &node.style;

        match node.raw_dom_data {
            NodeData::Document => "DOCUMENT",
            // NodeData::Doctype { .. } => return "DOCTYPE",
            NodeData::Text { .. } => node.node_debug_str().leak(),
            NodeData::Comment { .. } => "COMMENT",
            NodeData::AnonymousBlock(_) => "ANONYMOUS BLOCK",
            NodeData::Element(_) => {
                let display = match style.display {
                    Display::Flex => match style.flex_direction {
                        FlexDirection::Row | FlexDirection::RowReverse => "FLEX ROW",
                        FlexDirection::Column | FlexDirection::ColumnReverse => "FLEX COL",
                    },
                    Display::Grid => "GRID",
                    Display::Block => "BLOCK",
                    Display::None => "NONE",
                };
                format!("{} ({})", node.node_debug_str(), display).leak()
            } // NodeData::ProcessingInstruction { .. } => return "PROCESSING INSTRUCTION",
        }
    }

    fn get_final_layout(&self, node_id: NodeId) -> &Layout {
        &self.node_from_id(node_id).final_layout
    }
}

// pub struct ChildIter<'a>(std::slice::Iter<'a, usize>);
// impl<'a> Iterator for ChildIter<'a> {
//     type Item = NodeId;
//     fn next(&mut self) -> Option<Self::Item> {
//         self.0.next().copied().map(NodeId::from)
//     }
// }

pub struct RefCellChildIter<'a> {
    items: Ref<'a, [usize]>,
    idx: usize,
}
impl<'a> RefCellChildIter<'a> {
    fn new(items: Ref<'a, [usize]>) -> RefCellChildIter<'a> {
        RefCellChildIter { items, idx: 0 }
    }
}

impl Iterator for RefCellChildIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<Self::Item> {
        self.items.get(self.idx).map(|id| {
            self.idx += 1;
            NodeId::from(*id)
        })
    }
}
