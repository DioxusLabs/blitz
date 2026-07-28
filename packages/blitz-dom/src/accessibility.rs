use crate::{BaseDocument, Node as BlitzDomNode, local_name};
use accesskit::{Node as AccessKitNode, NodeId, Role, Tree, TreeId, TreeUpdate};

impl BaseDocument {
    pub fn build_accessibility_tree(&self) -> TreeUpdate {
        let mut nodes = std::collections::HashMap::new();
        let mut window = AccessKitNode::new(Role::Window);

        self.visit(|node_id, node| {
            let parent = node
                .parent
                .and_then(|parent_id| nodes.get_mut(&parent_id))
                .map(|(_, parent)| parent)
                .unwrap_or(&mut window);
            let (id, builder) = self.build_accessibility_node(node, parent);

            nodes.insert(node_id, (id, builder));
        });

        let mut nodes: Vec<_> = nodes
            .into_iter()
            .map(|(_, (id, node))| (id, node))
            .collect();
        nodes.push((NodeId(u64::MAX), window));

        let tree = Tree::new(NodeId(u64::MAX));
        TreeUpdate {
            tree_id: TreeId::ROOT,
            nodes,
            tree: Some(tree),
            focus: NodeId(self.focus_node_id.map(|id| id.as_u64()).unwrap_or(u64::MAX)),
        }
    }

    fn build_accessibility_node(
        &self,
        node: &BlitzDomNode,
        parent: &mut AccessKitNode,
    ) -> (NodeId, AccessKitNode) {
        let id = NodeId(node.id.as_u64());

        let mut builder = AccessKitNode::default();
        if node.parent.is_none() {
            builder.set_role(Role::Window)
        } else if let Some(element_data) = node.element_data() {
            let name = element_data.name.local.to_string();

            // <https://www.w3.org/TR/html-aam-1.0/>
            let role = match &*name {
                // Document structure
                "article" => Role::Article,
                "aside" => Role::Complementary,
                "footer" => Role::Footer,
                "header" => Role::Header,
                "main" => Role::Main,
                "nav" => Role::Navigation,
                "search" => Role::Search,
                "section" => Role::Section,
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Role::Heading,
                "p" => Role::Paragraph,
                "blockquote" => Role::Blockquote,
                "figure" => Role::Figure,
                "figcaption" | "caption" => Role::Caption,
                "hr" => Role::Splitter,

                // Grouping
                "ul" | "ol" | "menu" => Role::List,
                "li" => Role::ListItem,
                "dl" => Role::DescriptionList,
                "dt" => Role::Term,
                "dd" => Role::Definition,
                "dialog" => Role::Dialog,
                "fieldset" => Role::Group,
                "form" => Role::Form,
                "div" => Role::GenericContainer,

                // Tables
                "table" => Role::Table,
                "thead" | "tbody" | "tfoot" => Role::RowGroup,
                "tr" => Role::Row,
                "td" => Role::Cell,
                "th" => match element_data.attr(local_name!("scope")) {
                    Some("row") | Some("rowgroup") => Role::RowHeader,
                    _ => Role::ColumnHeader,
                },

                // Interactive
                // An <a> is only a link when it has an href.
                "a" => match element_data.attr(local_name!("href")) {
                    Some(_) => Role::Link,
                    None => Role::GenericContainer,
                },
                "button" => Role::Button,
                "label" => Role::Label,
                "legend" => Role::Label,
                "select" => match element_data.attr(local_name!("multiple")) {
                    Some(_) => Role::ListBox,
                    None => Role::ComboBox,
                },
                "option" => Role::ListBoxOption,
                "textarea" => Role::MultilineTextInput,
                "progress" => Role::ProgressIndicator,
                "meter" => Role::Meter,
                "output" => Role::Status,
                "summary" => Role::DisclosureTriangle,

                // Inline semantics
                "code" => Role::Code,
                "em" => Role::Emphasis,
                "strong" => Role::Strong,
                "mark" => Role::Mark,
                "time" => Role::Time,
                "img" => Role::Image,
                "iframe" => Role::Iframe,

                "input" => {
                    let ty = element_data.attr(local_name!("type")).unwrap_or("text");
                    match ty {
                        "button" | "submit" | "reset" => Role::Button,
                        "checkbox" => Role::CheckBox,
                        "color" => Role::ColorWell,
                        "date" => Role::DateInput,
                        "datetime-local" => Role::DateTimeInput,
                        "email" => Role::EmailInput,
                        "number" => Role::NumberInput,
                        "password" => Role::PasswordInput,
                        "radio" => Role::RadioButton,
                        "range" => Role::Slider,
                        "search" => Role::SearchInput,
                        "tel" => Role::PhoneNumberInput,
                        "time" => Role::TimeInput,
                        _ => Role::TextInput,
                    }
                }
                _ => Role::Unknown,
            };

            builder.set_role(role);
            builder.set_html_tag(name);
        } else if node.is_text_node() {
            builder.set_role(Role::TextRun);
            builder.set_value(node.text_content());
            parent.push_labelled_by(id)
        }

        parent.push_child(id);

        (id, builder)
    }
}
