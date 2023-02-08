use std::sync::{Arc, Mutex};

use dioxus_native_core::layout_attributes::apply_layout_attributes;
use dioxus_native_core::prelude::*;
use taffy::prelude::*;

use crate::text::TextContext;

#[derive(Clone, Default, Debug)]
pub struct TaffyLayout {
    pub style: Style,
    pub node: Option<Node>,
    pub layout: Option<Layout>,
}

impl PartialEq<Self> for TaffyLayout {
    fn eq(&self, other: &Self) -> bool {
        self.style == other.style && self.node == other.node
    }
}

impl Pass for TaffyLayout {
    type ChildDependencies = (Self,);
    type ParentDependencies = ();
    type NodeDependencies = ();

    const NODE_MASK: NodeMaskBuilder<'static> = NodeMaskBuilder::new()
        .with_attrs(AttributeMaskBuilder::All)
        .with_text();

    fn pass<'a>(
        &mut self,
        node_view: NodeView,
        _: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        _: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        children: Option<Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>>,
        ctx: &SendAnyMap,
    ) -> bool {
        let taffy: &Arc<Mutex<Taffy>> = ctx.get().unwrap();
        let text_context: &Arc<Mutex<TextContext>> = ctx.get().unwrap();
        let mut taffy = taffy.lock().unwrap();
        let mut changed = false;
        if let Some(text) = node_view.text() {
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

            for attr in node_view.attributes().unwrap() {
                let name = &attr.attribute.name;
                let value = attr.value;
                if let Some(value) = value.as_text() {
                    apply_layout_attributes(name, value, &mut style);
                }
            }

            // Set all direct nodes as our children
            let mut child_layout = vec![];
            for (l,) in children.into_iter().flatten() {
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

    fn create<'a>(
        node_view: NodeView<()>,
        node: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        parent: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        children: Option<Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>>,
        context: &SendAnyMap,
    ) -> Self {
        let mut myself = Self::default();
        myself.pass(node_view, node, parent, children, context);
        myself
    }
}
