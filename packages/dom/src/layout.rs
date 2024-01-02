//! Enable the dom to lay itself out using taffy
//!
//! In servo, style and layout happen together during traversal
//! However, in Blitz, we do a style pass then a layout pass.
//! This is slower, yes, but happens fast enough that it's not a huge issue.

use crate::{
    document::Document,
    node::Node,
    text::{text_measure_function, FontMetrics, TextContext, WritingMode},
};
use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_leaf_layout, compute_root_layout, prelude::*, round_layout, Cache, LayoutPartialTree,
    RoundTree, TraversePartialTree, TraverseTree,
};
use taffy::{
    prelude::{FlexDirection, NodeId},
    AvailableSpace, Dimension, Size, Style,
};

impl Document {
    fn node_from_id(&self, node_id: taffy::prelude::NodeId) -> &Node {
        &self.nodes[node_id.into()]
    }
    fn node_from_id_mut(&mut self, node_id: taffy::prelude::NodeId) -> &mut Node {
        &mut self.nodes[node_id.into()]
    }
}
impl TraverseTree for Document {}

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

            let font_metrics = FontMetrics {
                char_width: 32.0,
                char_height: 32.0,
            };

            match &node.node.data {
                markup5ever_rcdom::NodeData::Text { contents } => lay_text(
                    inputs,
                    &node.style,
                    contents.borrow().as_ref(),
                    &font_metrics,
                ),
                markup5ever_rcdom::NodeData::Element { name, attrs, .. } => {
                    // for the shadow root type elements

                    // todo: need to handle shadow roots
                    if name.local.as_ref() == "input" {
                        let value = attrs
                            .borrow()
                            .iter()
                            .find(|attr| attr.name.local.as_ref() == "value")
                            .map(|attr| attr.value.to_string());

                        if let Some(value) = value {
                            return lay_text(inputs, &node.style, &value, &font_metrics);
                        }
                    }
                    match node.style.display {
                        Display::Block => compute_block_layout(tree, node_id, inputs),
                        Display::Flex => compute_flexbox_layout(tree, node_id, inputs),
                        Display::Grid => compute_grid_layout(tree, node_id, inputs),
                        _ => todo!(),
                    }
                }
                markup5ever_rcdom::NodeData::Document => {
                    compute_block_layout(tree, node_id, inputs)
                }

                _ => todo!(),
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
    compute_leaf_layout(
        inputs,
        &node,
        Some(|known_dimensions, available_space| {
            let context = TextContext {
                text_content: contents.trim(),
                writing_mode: WritingMode::Horizontal,
            };
            text_measure_function(known_dimensions, available_space, &context, font_metrics)

            // Size {
            //     height: 200.0,
            //     width: 100.0,
            // }
        }),
    )
}

impl RoundTree for Document {
    fn get_unrounded_layout(&self, node_id: NodeId) -> &Layout {
        &self.node_from_id(node_id).unrounded_layout
    }

    fn set_final_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.node_from_id_mut(node_id).final_layout = *layout;
    }
}

pub struct ChildIter<'a>(std::slice::Iter<'a, usize>);
impl<'a> Iterator for ChildIter<'a> {
    type Item = NodeId;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied().map(NodeId::from)
    }
}
