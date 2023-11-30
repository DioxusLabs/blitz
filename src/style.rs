use crate::{node::NodeRef, RealDom, TaffyLayout};
use style;

impl RealDom {
    /// Resolve layout then style
    ///
    /// Uses Taffy's caching engine to improve performance between events.
    /// Is fed a list of dirty nodes from the virtualdom resolution so we know where to start.
    fn resolve(&mut self, dirty_nodes: Vec<NodeRef>) {}

    fn resolve_layout(&mut self) {}
}

/// A single node in the dom
///
/// Note that in blitz we use an ECS model to store nodes, but for the sake of this demo, we'll use a struct
pub struct Node {
    pub style: Style,
    pub layout: Option<taffy::node::Node>,
    pub text_context: TextContext,
}

impl Node {
    /// Resolve the layout for a single node in the dom
    fn resolve_node_layout(&mut self) -> bool {
        let mut changed: bool = false;
        if let Some(text) = node_view.text() {
            let mut text_context = &mut self.text_context;
            let font_size = fz.0;
            let (width, height) = text_context.get_text_size(None, font_size, text);

            let style = Style {
                size: Size {
                    height: Dimension::Points(height as f32),
                    width: Dimension::Points(width as f32),
                },
                ..Default::default()
            };

            let style_has_changed = self.style != style;

            if let Some(n) = self.node {
                if style_has_changed {
                    taffy.set_style(n, style.clone()).unwrap();
                    changed = true;
                }
            } else {
                self.node = Some(taffy.new_leaf(style.clone()).unwrap());
                changed = true;
            }

            if style_has_changed {
                self.style = style;
                changed = true;
            }
        } else {
            // gather up all the styles from the attribute list
            let mut style = Style::default();

            // Images default to a fixed size
            // TODO: The aspect ratio should be preserved when the image is scaled when box layout is implemented
            if let Some(image) = &image.0 {
                style = Style::default();
                style.size = Size {
                    width: Dimension::Points(image.width as f32),
                    height: Dimension::Points(image.height as f32),
                };
                style.flex_grow = 0.0;
                style.flex_shrink = 0.0;
            }

            for attr in node_view.attributes().into_iter().flatten() {
                let name = &attr.attribute.name;
                let value = attr.value;
                if let Some(value) = value.as_text() {
                    apply_layout_attributes(name, value, &mut style);
                }
            }

            // Set all direct nodes as our children
            let mut child_layout = vec![];
            for (l,) in children {
                child_layout.push(l.node.unwrap());
            }

            let style_has_changed = self.style != style;
            if let Some(n) = self.node {
                if taffy.children(n).unwrap() != child_layout {
                    taffy.set_children(n, &child_layout).unwrap();
                    changed = true;
                }
                if style_has_changed {
                    taffy.set_style(n, style.clone()).unwrap();
                    changed = true;
                }
            } else {
                self.node = Some(
                    taffy
                        .new_with_children(style.clone(), &child_layout)
                        .unwrap(),
                );
                changed = true;
            }

            if style_has_changed {
                self.style = style;
                changed = true;
            }
        }
        changed
    }
}
