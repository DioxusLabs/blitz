use std::cell::RefCell;
use std::rc::Rc;

use dioxus::core::{Attribute, ElementId};
use dioxus::native_core::layout_attributes::apply_layout_attributes;
use dioxus::native_core::node_ref::{AttributeMask, NodeMask};
use dioxus::native_core::state::ChildDepState;
use taffy::prelude::*;

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
    type Ctx = Rc<RefCell<Taffy>>;
    type DepState = Self;

    const NODE_MASK: NodeMask = NodeMask::new_with_attrs(AttributeMask::All).with_text();
    /// Setup the layout
    fn reduce<'a>(
        &mut self,
        node: dioxus::native_core::node_ref::NodeView,
        children: impl Iterator<Item = &'a Self::DepState>,
        ctx: &Self::Ctx,
    ) -> bool
    where
        Self::DepState: 'a,
    {
        let mut stretch = ctx.borrow_mut();
        let mut changed = false;
        if let Some(text) = node.text() {
            let char_len = text.chars().count();

            // todo: this should change with the font
            let style = Style {
                size: Size {
                    height: Dimension::Points(10.0),

                    width: Dimension::Points(char_len as f32 * 10.0),
                },
                ..Default::default()
            };

            if let Some(n) = self.node {
                if self.style != style {
                    stretch.set_style(n, style).unwrap();
                    changed = true;
                }
            } else {
                self.node = Some(stretch.new_node(style, &[]).unwrap());
                changed = true;
            }

            if style != self.style {
                self.style = style;
                changed = true;
            }
        } else {
            // gather up all the styles from the attribute list
            let mut style = Style::default();

            for Attribute { name, value, .. } in node.attributes() {
                if let Some(value) = value.as_text() {
                    apply_layout_attributes(name, value, &mut style);
                }
            }

            // the root node fills the entire area
            if node.id() == ElementId(0) {
                apply_layout_attributes("width", "100%", &mut style);
                apply_layout_attributes("height", "100%", &mut style);
            }

            // Set all direct nodes as our children
            let mut child_layout = vec![];
            for l in children {
                child_layout.push(l.node.unwrap());
            }

            if let Some(n) = self.node {
                if stretch.children(n).unwrap() != child_layout {
                    stretch.set_children(n, &child_layout).unwrap();
                    changed = true;
                }
                if self.style != style {
                    stretch.set_style(n, style).unwrap();
                    changed = true;
                }
            } else {
                self.node = Some(stretch.new_node(style, &child_layout).unwrap());
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
