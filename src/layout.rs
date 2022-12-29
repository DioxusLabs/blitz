use std::sync::{Arc, Mutex};

use dioxus::core::ElementId;
use dioxus_native_core::layout_attributes::apply_layout_attributes;
use dioxus_native_core::node_ref::{AttributeMask, NodeMask};
use dioxus_native_core::state::ChildDepState;
use taffy::prelude::*;

use crate::text::TextContext;

#[derive(Clone, Default, Debug)]
pub struct StretchLayout {
    pub style: Style,
    pub node: Option<Node>,
    pub layout: Option<Layout>,
}

impl PartialEq<Self> for StretchLayout {
    fn eq(&self, other: &Self) -> bool {
        self.style == other.style && self.node == other.node
    }
}

impl ChildDepState for StretchLayout {
    type Ctx = (Arc<Mutex<Taffy>>, Arc<Mutex<TextContext>>);
    type DepState = (Self,);

    const NODE_MASK: NodeMask = NodeMask::new_with_attrs(AttributeMask::All).with_text();
    /// Setup the layout
    fn reduce<'a>(
        &mut self,
        node: dioxus_native_core::node_ref::NodeView,
        children: impl Iterator<Item = (&'a Self,)>,
        (taffy, text_context): &Self::Ctx,
    ) -> bool
    where
        Self::DepState: 'a,
    {
        let mut taffy = taffy.lock().unwrap();
        let mut changed = false;
        if let Some(text) = node.text() {
            let mut text_context = text_context.lock().unwrap();
            let width = text_context.get_text_width(None, 16.0, text);

            let style = Style {
                size: Size {
                    height: Dimension::Points(16.0),

                    width: Dimension::Points(width as f32),
                },
                ..Default::default()
            };

            if let Some(n) = self.node {
                if self.style != style {
                    taffy.set_style(n, style).unwrap();
                    changed = true;
                }
            } else {
                self.node = Some(taffy.new_leaf(style).unwrap());
                changed = true;
            }

            if style != self.style {
                self.style = style;
                changed = true;
            }
        } else {
            // gather up all the styles from the attribute list
            let mut style = Style::default();

            for attr in node.attributes().unwrap() {
                let name = &attr.attribute.name;
                let value = attr.value;
                if let Some(value) = value.as_text() {
                    apply_layout_attributes(name, value, &mut style);
                }
            }

            // the root node fills the entire area
            if node.id() == Some(ElementId(0)) {
                apply_layout_attributes("width", "100%", &mut style);
                apply_layout_attributes("height", "100%", &mut style);
            }

            // Set all direct nodes as our children
            let mut child_layout = vec![];
            for (l,) in children {
                child_layout.push(l.node.unwrap());
            }

            if let Some(n) = self.node {
                if taffy.children(n).unwrap() != child_layout {
                    taffy.set_children(n, &child_layout).unwrap();
                    changed = true;
                }
                if self.style != style {
                    taffy.set_style(n, style).unwrap();
                    changed = true;
                }
            } else {
                self.node = Some(taffy.new_with_children(style, &child_layout).unwrap());
                changed = true;
            }

            if style != self.style {
                self.style = style;
                changed = true;
            }
        }
        changed
    }
}
