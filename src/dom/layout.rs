//! Enable the dom to lay itself out using taffy
//!
//! In servo, style and layout happen together during traversal
//! However, in Blitz, we do a style pass then a layout pass.
//! This is slower, yes, but happens fast enough that it's not a huge issue.

use crate::{styling::NodeData, Document};
use taffy::{prelude::NodeId, AvailableSpace, Dimension, Size, Style, TaffyTree};

impl Document {
    /// Walk the nodes now that they're properly styled and transfer their styles to the taffy style system
    /// Ideally we could just break apart the styles into ECS bits, but alas
    ///
    /// Todo: update taffy to use an associated type instead of slab key
    /// Todo: update taffy to support traited styles so we don't even need to rely on taffy for storage
    pub fn resolve_layout(&mut self) {
        self.layout.disable_rounding();

        let root = self.merge_dom(0, self.viewport.hidpi_scale, self.viewport.font_size);

        // We want the root container space to be auto unless specified
        // todo - root size should be allowed to expand past the borders.
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

    // todo: this is a dumb method and should be replaced with the taffy layouttree traits
    fn merge_dom(&mut self, node_id: usize, scale: f32, mut font_size: f32) -> NodeId {
        let data = &self.dom.nodes[node_id];

        // 1. merge what we can, if we have to
        let style = self.get_node_style(data, font_size, scale);

        // 2. Insert a leaf into taffy to associate with this node
        let leaf = self.layout.new_leaf(style.unwrap_or_default()).unwrap();
        data.layout_id.set(Some(leaf));

        // Cascade down the fontsize determined from stylo
        data.style.borrow().styles.get_primary().map(|primary| {
            // todo: cache this bs on the text node itself
            use style::values::generics::transform::ToAbsoluteLength;
            font_size = primary
                .clone_font_size()
                .computed_size()
                .to_pixel_length(None)
                .unwrap();
        });

        // 3. walk to to children and merge them too
        // Need to dance around the borrow checker, unfortunately
        for x in 0..data.children.len() {
            let child_id = self.dom.nodes[node_id].children[x];
            let child_layout = self.merge_dom(child_id, scale, font_size);
            self.layout.add_child(leaf, child_layout).unwrap();
        }

        leaf
    }

    fn get_node_style(&self, data: &NodeData, font_size: f32, scale: f32) -> Option<Style> {
        use markup5ever_rcdom::NodeData;

        match &data.node.data {
            // need to add a measure function?
            NodeData::Text { contents } => {
                let (width, height) = self.text_context.get_text_size(
                    None,
                    font_size * scale,
                    contents.borrow().as_ref(),
                );

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

fn translate_stylo_to_taffy(data: &NodeData) -> Style {
    let style_data = data.style.borrow();
    let primary = style_data.styles.primary();

    let style = Style::DEFAULT;

    let _box = primary.get_box();

    style
}
