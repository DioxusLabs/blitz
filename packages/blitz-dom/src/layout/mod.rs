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
    compute_leaf_layout, prelude::*, FlexDirection, LayoutPartialTree, NodeId, ResolveOrZero,
    RoundTree, Style, TraversePartialTree, TraverseTree,
};

pub(crate) mod construct;
pub(crate) mod inline;
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

    fn get_core_container_style(&self, node_id: NodeId) -> &Style {
        &self.node_from_id(node_id).style
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.node_from_id_mut(node_id).unrounded_layout = *layout;
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

                    // TODO: deduplicate with single-line text input
                    if *element_data.name.local == *"textarea" {
                        return compute_leaf_layout(
                            inputs,
                            &node.style,
                            |_known_size, _available_space| taffy::Size {
                                width: 300.0,
                                height: resolved_line_height.unwrap_or(16.0) * 4.0,
                            },
                        );
                    }

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

                    if *element_data.name.local == *"img"
                        || (cfg!(feature = "svg") && *element_data.name.local == *"svg")
                    {
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
                            #[cfg(feature = "svg")]
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
                        return tree.compute_inline_layout(usize::from(node_id), inputs);
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

impl taffy::CacheTree for Document {
    #[inline]
    fn cache_get(
        &self,
        node_id: NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: taffy::RunMode,
    ) -> Option<taffy::LayoutOutput> {
        self.node_from_id(node_id)
            .cache
            .get(known_dimensions, available_space, run_mode)
    }

    #[inline]
    fn cache_store(
        &mut self,
        node_id: NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: taffy::RunMode,
        layout_output: taffy::LayoutOutput,
    ) {
        self.node_from_id_mut(node_id).cache.store(
            known_dimensions,
            available_space,
            run_mode,
            layout_output,
        );
    }

    #[inline]
    fn cache_clear(&mut self, node_id: NodeId) {
        self.node_from_id_mut(node_id).cache.clear();
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
