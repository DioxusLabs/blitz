use crate::{local_name, BaseDocument, Node};
use accesskit::{NodeBuilder, NodeId, Role, Tree, TreeUpdate};

impl BaseDocument {
    pub fn build_accessibility_tree(&self) -> TreeUpdate {
        let mut nodes = std::collections::HashMap::new();
        let mut window = NodeBuilder::new(Role::Window);

        self.visit(|node_id, node| {
            let parent = node
                .parent
                .and_then(|parent_id| nodes.get_mut(&parent_id))
                .map(|(_, parent)| parent)
                .unwrap_or(&mut window);
            let (id, node_builder) = self.build_accessibility_node(node, parent);

            nodes.insert(node_id, (id, node_builder));
        });

        let mut nodes: Vec<_> = nodes
            .into_iter()
            .map(|(_, (id, node))| (id, node.build()))
            .collect();
        nodes.push((NodeId(0), window.build()));

        let tree = Tree::new(NodeId(0));
        TreeUpdate {
            nodes,
            tree: Some(tree),
            focus: NodeId(0),
        }
    }

    fn build_accessibility_node(
        &self,
        node: &Node,
        parent: &mut NodeBuilder,
    ) -> (NodeId, NodeBuilder) {
        let id = NodeId(node.id as u64);

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
                        "checkbox" => Role::CheckBox,
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
