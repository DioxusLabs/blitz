//! Enable the dom to lay itself out using taffy
//!
//! In servo, style and layout happen together during traversal
//! However, in Blitz, we do a style pass then a layout pass.
//! This is slower, yes, but happens fast enough that it's not a huge issue.

use crate::{
    styling::{BlitzNode, NodeData},
    text::TextContext,
    Document,
};
use taffy::{prelude::NodeId, AvailableSpace, Dimension, Size, Style, TaffyTree};

impl Document {
    /// Walk the nodes now that they're properly styled and transfer their styles to the taffy style system
    /// Ideally we could just break apart the styles into ECS bits, but alas
    ///
    /// Todo: update taffy to use an associated type instead of slab key
    /// Todo: update taffy to support traited styles so we don't even need to rely on taffy for storage
    pub fn resolve_layout(&mut self) {
        let root = merge_dom(&mut self.layout, &mut self.text, self.dom.root_element());

        // We want the root container space to be auto
        let root_size = Size {
            width: Dimension::Auto,
            height: Dimension::Auto,
        };

        // But we want the layout space to be the viewport (so things like 100vh make sense)
        // In reality, we'll want the layout size to bleed through beyond the size of the container
        // The container should not be sized unless explicitly set
        let available = Size {
            width: AvailableSpace::Definite(self.viewport.window_size.width as _),
            height: AvailableSpace::Definite(self.viewport.window_size.height as _),
        };

        let style = Style {
            size: root_size,
            ..self.layout.style(root).unwrap().clone()
        };

        self.layout.set_style(root, style).unwrap();
        self.layout.compute_layout(root, available).unwrap();
    }
}

fn merge_dom(taffy: &mut TaffyTree, text_context: &mut TextContext, node: BlitzNode) -> NodeId {
    let data = node.data();

    // 1. merge what we can, if we have to
    use markup5ever_rcdom::NodeData;
    let style = match &data.node.data {
        // need to add a measure function?
        NodeData::Text { contents } => {
            let text = contents.borrow();
            let font_size = 80.0;
            let (width, height) = text_context.get_text_size(None, font_size, text.as_ref());

            let style = Style {
                size: Size {
                    height: Dimension::Length(height as f32),
                    width: Dimension::Length(width as f32),
                },
                ..Default::default()
            };
            Some(style)
        }

        // merge element via its attrs
        NodeData::Element { name, attrs, .. } => {
            // Get the stylo data for this
            Some(translate_stylo_to_taffy(node, data))
        }

        NodeData::Document
        | NodeData::Doctype { .. }
        | NodeData::Comment { .. }
        | NodeData::ProcessingInstruction { .. } => None,
    };

    // 2. Insert a leaf into taffy to associate with this node
    let leaf = taffy.new_leaf(style.unwrap_or_default()).unwrap();
    data.layout_id.set(Some(leaf));

    // 3. walk to to children and merge them too
    for idx in data.children.iter() {
        let child = node.with(*idx);
        let child_layout = merge_dom(taffy, text_context, child);
        taffy.add_child(leaf, child_layout).unwrap();
    }

    leaf
}

fn translate_stylo_to_taffy(node: BlitzNode, data: &NodeData) -> Style {
    let style_data = data.style.borrow();
    let primary = style_data.styles.primary();

    let mut style = Style::DEFAULT;

    let _box = primary.get_box();

    style
}
