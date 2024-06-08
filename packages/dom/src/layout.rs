//! Enable the dom to lay itself out using taffy
//!
//! In servo, style and layout happen together during traversal
//! However, in Blitz, we do a style pass then a layout pass.
//! This is slower, yes, but happens fast enough that it's not a huge issue.

use crate::node::NodeData;
use crate::{
    document::Document,
    image::{image_measure_function, ImageContext},
    node::Node,
    text::{text_measure_function, FontMetrics, TextContext, WritingMode},
};
use html5ever::local_name;
use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_leaf_layout, prelude::*, Cache, Dimension, FlexDirection, LayoutPartialTree, NodeId,
    RoundTree, Size, Style, TraversePartialTree, TraverseTree,
};

impl Document {
    fn node_from_id(&self, node_id: taffy::prelude::NodeId) -> &Node {
        &self.nodes[node_id.into()]
    }
    fn node_from_id_mut(&mut self, node_id: taffy::prelude::NodeId) -> &mut Node {
        &mut self.nodes[node_id.into()]
    }
}

impl TraversePartialTree for Document {
    type ChildIter<'a> = ChildIter<'a>;

    fn child_ids(&self, node_id: NodeId) -> Self::ChildIter<'_> {
        ChildIter(self.node_from_id(node_id).children.iter())
    }

    fn child_count(&self, node_id: NodeId) -> usize {
        self.node_from_id(node_id).children.len()
    }

    fn get_child_id(&self, node_id: NodeId, index: usize) -> NodeId {
        NodeId::from(self.node_from_id(node_id).children[index])
    }
}
impl TraverseTree for Document {}

impl LayoutPartialTree for Document {
    fn get_style(&self, node_id: NodeId) -> &Style {
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
            let node = tree.node_from_id_mut(node_id);

            // this is because of hidpi affecting the layout - we need to propagate this down
            // the 2.0 is the dpi and the .2 is a bit extra since our measure function is a bit off
            const FUDGE_FACTOR: f32 = 2.2;

            let font_metrics = FontMetrics {
                char_width: 8.0 * FUDGE_FACTOR,
                char_height: 16.0 * FUDGE_FACTOR,
            };

            match &node.raw_dom_data {
                NodeData::Text(data) => lay_text(inputs, &node.style, &data.content, &font_metrics),
                NodeData::Element(element_data) => {
                    // Hide hidden nodes
                    if let Some(value) = node.attr(local_name!("hidden")) {
                        if value == "hidden" || value == "" {
                            node.style.display = Display::None;
                            return taffy::LayoutOutput::HIDDEN;
                        }
                    }

                    // todo: need to handle shadow roots by actually descending into them
                    if *element_data.name.local == *"input" {
                        // if the input type is hidden, hide it
                        if let Some(value) = node.attr(local_name!("type")) {
                            if value == "hidden" {
                                node.style.display = Display::None;
                                return taffy::LayoutOutput::HIDDEN;
                            }
                        }
                    }

                    if *element_data.name.local == *"img" {
                        node.style.min_size = Size {
                            width: Dimension::Length(0.0),
                            height: Dimension::Length(0.0),
                        };
                        node.style.display = Display::Block;

                        // Get image's native size
                        let image_data = match &element_data.image {
                            Some(image) => ImageContext {
                                width: image.width() as f32,
                                height: image.height() as f32,
                            },
                            None => ImageContext {
                                width: 0.0,
                                height: 0.0,
                            },
                        };

                        return compute_leaf_layout(
                            inputs,
                            &node.style,
                            |known_dimensions, _available_space| {
                                image_measure_function(
                                    known_dimensions,
                                    inputs.parent_size,
                                    &image_data,
                                    &node.style,
                                )
                            },
                        );
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

fn lay_text(
    inputs: taffy::LayoutInput,
    node: &Style,
    contents: &str,
    font_metrics: &FontMetrics,
) -> taffy::LayoutOutput {
    compute_leaf_layout(inputs, &node, |known_dimensions, available_space| {
        let context = TextContext {
            text_content: contents.trim(),
            writing_mode: WritingMode::Horizontal,
        };
        text_measure_function(known_dimensions, available_space, &context, font_metrics)
    })
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
            NodeData::Document => return "DOCUMENT",
            // NodeData::Doctype { .. } => return "DOCTYPE",
            NodeData::Text { .. } => return node.node_debug_str().leak(),
            NodeData::Comment { .. } => return "COMMENT",
            NodeData::Element { .. } => {
                let display = match style.display {
                    Display::Flex => match style.flex_direction {
                        FlexDirection::Row | FlexDirection::RowReverse => "FLEX ROW",
                        FlexDirection::Column | FlexDirection::ColumnReverse => "FLEX COL",
                    },
                    Display::Grid => "GRID",
                    Display::Block => "BLOCK",
                    Display::None => "NONE",
                };
                return format!("{} ({})", node.node_debug_str(), display).leak();
            } // NodeData::ProcessingInstruction { .. } => return "PROCESSING INSTRUCTION",
        };
    }

    fn get_final_layout(&self, node_id: NodeId) -> &Layout {
        &self.node_from_id(node_id).final_layout
    }
}

pub struct ChildIter<'a>(std::slice::Iter<'a, usize>);
impl<'a> Iterator for ChildIter<'a> {
    type Item = NodeId;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied().map(NodeId::from)
    }
}
