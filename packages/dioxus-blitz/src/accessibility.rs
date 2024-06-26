use crate::waker::UserEvent;
use accesskit::Role;
use blitz_dom::{local_name, Document, Node};
use winit::{event_loop::EventLoopProxy, window::Window};

/// State of the accessibility node tree and platform adapter.
pub struct AccessibilityState {
    /// Adapter to connect to the [`EventLoop`](`winit::event_loop::EventLoop`).
    adapter: accesskit_winit::Adapter,

    /// Next ID to assign an an [`accesskit::Node`].
    next_id: u64,
}

impl AccessibilityState {
    pub fn new(window: &Window, proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            adapter: accesskit_winit::Adapter::with_event_loop_proxy(window, proxy.clone()),
            next_id: 1,
        }
    }
    pub fn build_tree(&mut self, doc: &Document) {
        let mut nodes = std::collections::HashMap::new();
        let mut window = accesskit::NodeBuilder::new(accesskit::Role::Window);

        doc.visit(|node_id, node| {
            let (id, node_builder) = self.build_node(node);

            if let Some(parent_id) = node.parent {
                let (_, parent_node): &mut (_, accesskit::NodeBuilder) =
                    nodes.get_mut(&parent_id).unwrap();
                parent_node.push_child(id)
            } else {
                window.push_child(id)
            }

            nodes.insert(node_id, (id, node_builder));
        });

        let mut nodes: Vec<_> = nodes
            .into_iter()
            .map(|(_, (id, node))| (id, node.build()))
            .collect();
        nodes.push((accesskit::NodeId(0), window.build()));

        let tree = accesskit::Tree::new(accesskit::NodeId(0));
        let tree_update = accesskit::TreeUpdate {
            nodes,
            tree: Some(tree),
            focus: accesskit::NodeId(0),
        };

        self.adapter.update_if_active(|| tree_update)
    }

    #[cfg(feature = "accessibility")]
    fn build_node(&mut self, node: &Node) -> (accesskit::NodeId, accesskit::NodeBuilder) {
        let mut node_builder = accesskit::NodeBuilder::default();
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
            node_builder.set_name(name);
        } else if node.is_text_node() {
            node_builder.set_role(accesskit::Role::StaticText);
            node_builder.set_name(node.text_content());
        }

        let id = accesskit::NodeId(self.next_id);
        self.next_id += 1;

        (id, node_builder)
    }
}
