//! Enable the dom to lay itself out using taffy
//!
//! In servo, style and layout happen together during traversal
//! However, in Blitz, we do a style pass then a layout pass.
//! This is slower, yes, but happens fast enough that it's not a huge issue.

use crate::Document;
use taffy::{
    prelude::{FlexDirection, NodeId},
    AvailableSpace, Dimension, Size, Style,
};

impl Document {
    /// Walk the nodes now that they're properly styled and transfer their styles to the taffy style system
    /// Ideally we could just break apart the styles into ECS bits, but alas
    ///
    /// Todo: update taffy to use an associated type instead of slab key
    /// Todo: update taffy to support traited styles so we don't even need to rely on taffy for storage
    pub fn resolve_layout(&mut self) {
        self.taffy.disable_rounding();

        let layout = Style {
            flex_direction: FlexDirection::Column,
            size: Size {
                width: Dimension::Auto,
                height: Dimension::Auto,
            },
            ..Default::default()
        };

        let root = self.taffy.new_leaf(layout).unwrap();

        self.merge_dom(0, Some(root), self.viewport.font_size);

        // But we want the layout space to be the viewport (so things like 100vh make sense)
        // In reality, we'll want the layout size to bleed through beyond the size of the container
        // The container should not be sized unless explicitly set
        let available_space = Size {
            width: AvailableSpace::Definite(self.viewport.window_size.width as _),
            height: AvailableSpace::Definite(self.viewport.window_size.height as _),
        };

        self.taffy.compute_layout(root, available_space).unwrap();
    }

    // todo: this is a dumb method and should be replaced with the taffy layouttree traits
    fn merge_dom(&mut self, node_id: usize, parent: Option<NodeId>, mut font_size: f32) {
        let data = &self.dom.nodes[node_id];

        match &data.node.data {
            markup5ever_rcdom::NodeData::Element { name, .. } => {
                // skip head nodes/script nodes
                // these are handled elsewhere...
                match name.local.as_ref() {
                    "style" | "head" | "script" => {
                        if name.local.as_ref() == "style" {
                            let contents = &self.dom.nodes[data.children[0]];

                            for child in &data.children {
                                println!("{:?}", self.dom.nodes[*child]);
                            }

                            match &contents.node.data {
                                markup5ever_rcdom::NodeData::Text { contents } => {
                                    let contents = contents.clone();
                                    self.add_stylesheet(contents.borrow().as_ref());
                                }
                                _ => panic!("{data:?}"),
                            }

                            // self.add_stylesheet(css)
                            // self.stylist.append_stylesheet(sheet, &self.dom.guard.read());
                        }
                        return;
                    }
                    _ => {}
                }
            }
            markup5ever_rcdom::NodeData::Document => {}
            markup5ever_rcdom::NodeData::Doctype { .. } => {}
            markup5ever_rcdom::NodeData::Text { .. } => {}
            markup5ever_rcdom::NodeData::Comment { .. } => return,
            markup5ever_rcdom::NodeData::ProcessingInstruction { .. } => return,
        }

        // 1. merge what we can, if we have to
        let style = self.get_node_style(data, font_size);

        // 2. Insert a leaf into taffy to associate with this node
        let leaf = self.taffy.new_leaf(style.unwrap_or_default()).unwrap();
        data.layout_id.set(Some(leaf));

        // Cascade down the fontsize determined from stylo
        data.style
            .borrow()
            .styles
            .get_primary()
            .map(|primary| font_size = primary.clone_font_size().computed_size().px());

        // Attach this node to its parent
        if let Some(parent) = parent {
            _ = self.taffy.add_child(parent, leaf);
        }

        // 3. walk to to children and merge them too
        // Need to dance around the borrow checker, unfortunately
        for x in 0..data.children.len() {
            let child_id = self.dom.nodes[node_id].children[x];
            self.merge_dom(child_id, Some(leaf), font_size);
        }
    }

    fn get_node_style(&self, data: &crate::styling::NodeData, font_size: f32) -> Option<Style> {
        use markup5ever_rcdom::NodeData;

        match &data.node.data {
            // need to add a measure function?
            NodeData::Text { contents } => {
                let (width, height) = self.text_context.get_text_size(
                    None,
                    font_size * self.viewport.scale(),
                    contents.borrow().as_ref(),
                );

                let style = Style {
                    size: Size {
                        height: Dimension::Length(height as f32),
                        width: Dimension::Length(width as f32),
                    },
                    // padding: taffy::Rect {
                    //     left: taffy::prelude::LengthPercentage::Length(50.0),
                    //     right: taffy::prelude::LengthPercentage::Length(50.0),
                    //     top: taffy::prelude::LengthPercentage::Length(50.0),
                    //     bottom: taffy::prelude::LengthPercentage::Length(50.0),
                    // },
                    margin: taffy::Rect {
                        left: taffy::prelude::LengthPercentageAuto::Length(0.0),
                        right: taffy::prelude::LengthPercentageAuto::Length(0.0),
                        top: taffy::prelude::LengthPercentageAuto::Length(0.0),
                        bottom: taffy::prelude::LengthPercentageAuto::Length(0.0),
                    },
                    ..Default::default()
                };
                Some(style)
            }

            // merge element via its attrs
            NodeData::Element { .. } => {
                // Get the stylo data for this
                Some(translate_stylo_to_taffy(data))
            }

            NodeData::Document
            | NodeData::Doctype { .. }
            | NodeData::Comment { .. }
            | NodeData::ProcessingInstruction { .. } => None,
        }
    }
}

fn translate_stylo_to_taffy(data: &crate::styling::NodeData) -> Style {
    let style_data = data.style.borrow();
    let primary = style_data.styles.primary();

    let style = Style::DEFAULT;

    let _box = primary.get_box();

    style
}
