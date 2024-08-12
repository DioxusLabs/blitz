use crate::waker::BlitzEvent;
use accesskit::{NodeBuilder, NodeId, Role, Tree, TreeUpdate};
use blitz_dom::{local_name, Document, Node};
use std::collections::HashMap;
use winit::{event_loop::EventLoopProxy, window::Window};

/// State of the accessibility node tree and platform adapter.
pub struct AccessibilityState {
    /// Adapter to connect to the [`EventLoop`](`winit::event_loop::EventLoop`).
    pub adapter: accesskit_winit::Adapter,
}

impl AccessibilityState {
    pub fn new(window: &Window, proxy: EventLoopProxy<BlitzEvent>) -> Self {
        Self {
            adapter: accesskit_winit::Adapter::with_event_loop_proxy(window, proxy.clone()),
        }
    }

    pub fn build_tree(&mut self, doc: &Document) {
        let mut nodes = HashMap::new();
        let mut window = NodeBuilder::new(Role::Window);

        doc.visit(|node_id, node| {
            let parent = node
                .parent
                .and_then(|parent_id| nodes.get_mut(&parent_id))
                .map(|(_, parent)| parent)
                .unwrap_or(&mut window);
            let (id, node_builder) = self.build_node(NodeId(node_id as _), node, parent);

            nodes.insert(node_id, (id, node_builder));
        });

        let mut nodes: Vec<_> = nodes
            .into_iter()
            .map(|(_, (id, node))| (id, node.build()))
            .collect();
        nodes.push((NodeId(0), window.build()));

        let tree = Tree::new(NodeId(0));
        let tree_update = TreeUpdate {
            nodes,
            tree: Some(tree),
            focus: NodeId(doc.focus().unwrap_or_default() as _),
        };

        self.adapter.update_if_active(|| tree_update)
    }

    fn build_node(
        &mut self,
        id: NodeId,
        node: &Node,
        parent: &mut NodeBuilder,
    ) -> (NodeId, NodeBuilder) {
        let mut node_builder = NodeBuilder::default();

        if let Some(element_data) = node.element_data() {
            let name = element_data.name.local.to_string();

            // TODO match more roles
            let role = match &*name {
                "button" => Role::Button,
                "div" => Role::GenericContainer,
                "header" => Role::Header,
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Role::Heading,
                "p" => Role::Paragraph,
                "section" => Role::Section,
                "input" => {
                    let ty = element_data.attr(local_name!("type")).unwrap_or("text");
                    match ty {
                        "number" => Role::NumberInput,
                        _ => Role::TextInput,
                    }
                }
                _ => Role::Unknown,
            };

            node_builder.set_role(role);
            node_builder.set_html_tag(name);
        } else if node.is_text_node() {
            node_builder.set_role(Role::StaticText);
            node_builder.set_name(node.text_content());
            parent.push_labelled_by(id)
        }

        parent.push_child(id);

        (id, node_builder)
    }
}
