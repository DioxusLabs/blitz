use crate::{BaseDocument, ElementData, Node as BlitzDomNode, local_name};
use accesskit::{Node as AccessKitNode, NodeId, Rect, Role, Tree, TreeId, TreeUpdate};
use style::properties::longhands::visibility;

impl BaseDocument {
    pub fn build_accessibility_tree(&self) -> TreeUpdate {
        let mut nodes = std::collections::HashMap::new();
        let mut window = AccessKitNode::new(Role::Window);
        let mut hidden_nodes = std::collections::HashSet::new();

        self.visit(|node_id, node| {
            if node.is_hidden_from_accessibility_tree()
                || node
                    .parent
                    .map(|p| hidden_nodes.contains(&p))
                    .unwrap_or(false)
            {
                hidden_nodes.insert(node_id);
                return;
            }
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
        if let Some(bounds) = self.accessibility_bounds(node) {
            builder.set_bounds(bounds);
        }
        if node.parent.is_none() {
            builder.set_role(Role::Window)
        } else if let Some(element_data) = node.element_data() {
            let name = element_data.name.local.to_string();
            let role_attr = element_data.attr(local_name!("role"));

            // TODO: The roles of elements with strong native semantics cannot be overridden; see
            // https://www.w3.org/TR/wai-aria-1.2/#host_general_conflict.
            let role = role_attr
                .and_then(role_from_name)
                .or_else(|| role_from_element_data(element_data))
                .unwrap_or(Role::Unknown);

            builder.set_role(role);
            builder.set_html_tag(name);

            // https://www.w3.org/TR/wai-aria-1.2/#tree_exclusion
            if element_data.attr(local_name!("aria-hidden")) == Some("true") {
                builder.set_hidden();
            }
        } else if node.is_text_node() {
            builder.set_role(Role::TextRun);
            builder.set_value(node.text_content());
            parent.push_labelled_by(id)
        }

        parent.push_child(id);

        (id, builder)
    }

    /// The node's bounding box in physical pixels relative to the viewport origin,
    /// which is the coordinate space AccessKit expects for `bounds`. Text nodes have
    /// no layout box of their own, so they use their parent element's bounds.
    fn accessibility_bounds(&self, node: &BlitzDomNode) -> Option<Rect> {
        let node_id = if node.is_text_node() {
            node.parent?
        } else if node.is_element() || node.parent.is_none() {
            node.id
        } else {
            return None;
        };
        let rect = self.get_client_bounding_rect(node_id)?;
        let scale = self.viewport().scale_f64();
        Some(Rect::new(
            rect.x * scale,
            rect.y * scale,
            (rect.x + rect.width) * scale,
            (rect.y + rect.height) * scale,
        ))
    }
}

impl BlitzDomNode {
    // https://www.w3.org/TR/wai-aria-1.2/#tree_exclusion
    fn is_hidden_from_accessibility_tree(&self) -> bool {
        self.try_stylo_element_data()
            .as_ref()
            .and_then(|s| s.get())
            .map(|s| {
                s.styles.is_display_none()
                    || s.styles.primary().clone_visibility()
                        == visibility::computed_value::T::Hidden
            })
            .unwrap_or(false)
    }
}

fn role_from_name(name: &str) -> Option<Role> {
    match name {
        "alert" => Some(Role::Alert),
        "alertdialog" => Some(Role::AlertDialog),
        "button" => Some(Role::Button),
        "checkbox" => Some(Role::CheckBox),
        "dialog" => Some(Role::Dialog),
        "gridcell" => Some(Role::GridCell),
        "link" => Some(Role::Link),
        "log" => Some(Role::Log),
        "marquee" => Some(Role::Marquee),
        "menuitem" => Some(Role::MenuItem),
        "menuitemcheckbox" => Some(Role::MenuItemCheckBox),
        "menuitemradio" => Some(Role::MenuItemRadio),
        "option" => Some(Role::ListBoxOption),
        "progressbar" => Some(Role::ProgressIndicator),
        "radio" => Some(Role::RadioButton),
        "scrollbar" => Some(Role::ScrollBar),
        "slider" => Some(Role::Slider),
        "spinbutton" => Some(Role::SpinButton),
        "status" => Some(Role::Status),
        "tab" => Some(Role::Tab),
        "tabpanel" => Some(Role::TabPanel),
        "textbox" => Some(Role::TextInput),
        "timer" => Some(Role::Timer),
        "tooltip" => Some(Role::Tooltip),
        "treeitem" => Some(Role::TreeItem),
        "combobox" => Some(Role::ComboBox),
        "grid" => Some(Role::Grid),
        "listbox" => Some(Role::ListBox),
        "menu" => Some(Role::Menu),
        "menubar" => Some(Role::MenuBar),
        "radiogroup" => Some(Role::RadioGroup),
        "tablist" => Some(Role::TabList),
        "tree" => Some(Role::Tree),
        "treegrid" => Some(Role::TreeGrid),
        "article" => Some(Role::Article),
        "columnheader" => Some(Role::ColumnHeader),
        "definition" => Some(Role::Definition),
        "document" => Some(Role::Document),
        "group" => Some(Role::Group),
        "heading" => Some(Role::Heading),
        "img" => Some(Role::Image),
        "list" => Some(Role::List),
        "listitem" => Some(Role::ListItem),
        "math" => Some(Role::Math),
        "note" => Some(Role::Note),
        "region" => Some(Role::Region),
        "row" => Some(Role::Row),
        "rowgroup" => Some(Role::RowGroup),
        "rowheader" => Some(Role::RowHeader),
        "toolbar" => Some(Role::Toolbar),
        "application" => Some(Role::Application),
        "banner" => Some(Role::Banner),
        "complementary" => Some(Role::Complementary),
        "contentinfo" => Some(Role::ContentInfo),
        "form" => Some(Role::Form),
        "main" => Some(Role::Main),
        "navigation" => Some(Role::Navigation),
        "search" => Some(Role::Search),
        _ => None,
    }
}

fn role_from_element_data(element_data: &ElementData) -> Option<Role> {
    // <https://www.w3.org/TR/html-aam-1.0/>
    match &*element_data.name.local {
        // Document structure
        "article" => Some(Role::Article),
        "aside" => Some(Role::Complementary),
        "footer" => Some(Role::Footer),
        "header" => Some(Role::Header),
        "main" => Some(Role::Main),
        "nav" => Some(Role::Navigation),
        "search" => Some(Role::Search),
        "section" => Some(Role::Section),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(Role::Heading),
        "p" => Some(Role::Paragraph),
        "blockquote" => Some(Role::Blockquote),
        "figure" => Some(Role::Figure),
        "figcaption" | "caption" => Some(Role::Caption),
        "hr" => Some(Role::Splitter),

        // Grouping
        "ul" | "ol" | "menu" => Some(Role::List),
        "li" => Some(Role::ListItem),
        "dl" => Some(Role::DescriptionList),
        "dt" => Some(Role::Term),
        "dd" => Some(Role::Definition),
        "dialog" => Some(Role::Dialog),
        "fieldset" => Some(Role::Group),
        "form" => Some(Role::Form),
        "div" => Some(Role::GenericContainer),

        // Tables
        "table" => Some(Role::Table),
        "thead" | "tbody" | "tfoot" => Some(Role::RowGroup),
        "tr" => Some(Role::Row),
        "td" => Some(Role::Cell),
        "th" => match element_data.attr(local_name!("scope")) {
            Some("row") | Some("rowgroup") => Some(Role::RowHeader),
            _ => Some(Role::ColumnHeader),
        },

        // Interactive
        // An <a> is only a link when it has an href.
        "a" => match element_data.attr(local_name!("href")) {
            Some(_) => Some(Role::Link),
            None => Some(Role::GenericContainer),
        },
        "button" => Some(Role::Button),
        "label" => Some(Role::Label),
        "legend" => Some(Role::Label),
        "select" => match element_data.attr(local_name!("multiple")) {
            Some(_) => Some(Role::ListBox),
            None => Some(Role::ComboBox),
        },
        "option" => Some(Role::ListBoxOption),
        "textarea" => Some(Role::MultilineTextInput),
        "progress" => Some(Role::ProgressIndicator),
        "meter" => Some(Role::Meter),
        "output" => Some(Role::Status),
        "summary" => Some(Role::DisclosureTriangle),

        // Inline semantics
        "code" => Some(Role::Code),
        "em" => Some(Role::Emphasis),
        "strong" => Some(Role::Strong),
        "mark" => Some(Role::Mark),
        "time" => Some(Role::Time),
        "img" => Some(Role::Image),
        "iframe" => Some(Role::Iframe),

        "input" => {
            let ty = element_data.attr(local_name!("type")).unwrap_or("text");
            match ty {
                "button" | "submit" | "reset" => Some(Role::Button),
                "checkbox" => Some(Role::CheckBox),
                "color" => Some(Role::ColorWell),
                "date" => Some(Role::DateInput),
                "datetime-local" => Some(Role::DateTimeInput),
                "email" => Some(Role::EmailInput),
                "number" => Some(Role::NumberInput),
                "password" => Some(Role::PasswordInput),
                "radio" => Some(Role::RadioButton),
                "range" => Some(Role::Slider),
                "search" => Some(Role::SearchInput),
                "tel" => Some(Role::PhoneNumberInput),
                "time" => Some(Role::TimeInput),
                _ => Some(Role::TextInput),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{BaseDocument, DocumentConfig, qual_name};
    use accesskit::NodeId;
    use blitz_traits::shell::{ColorScheme, Viewport};

    #[test]
    fn nodes_have_layout_bounds() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 2.0, ColorScheme::Light)),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(
            "html, body, div { display: block; } body { margin: 0; } \
             div { margin-left: 10px; margin-top: 20px; width: 300px; height: 100px; }",
        );
        let root_id = doc.root_node().id;

        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(qual_name!("body"), vec![]);
        let div = mutator.create_element(qual_name!("div"), vec![]);
        let text = mutator.create_text_node("some text");
        mutator.append_children(div, &[text]);
        mutator.append_children(body, &[div]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);
        doc.resolve(0.0);

        let update = doc.build_accessibility_tree();
        let find = |id: crate::NodeId| {
            update
                .nodes
                .iter()
                .find(|(n, _)| *n == NodeId(id.as_u64()))
                .map(|(_, node)| node)
                .unwrap()
        };

        // Bounds are in physical pixels (scale factor 2.0).
        let div_bounds = find(div).bounds().unwrap();
        assert_eq!(div_bounds.x0, 20.0);
        assert_eq!(div_bounds.y0, 40.0);
        assert_eq!(div_bounds.width(), 600.0);
        assert_eq!(div_bounds.height(), 200.0);

        // Text nodes inherit their parent element's bounds.
        assert_eq!(find(text).bounds().unwrap(), div_bounds);

        let body_bounds = find(body).bounds().unwrap();
        assert_eq!(body_bounds.x0, 0.0);
        assert_eq!(body_bounds.width(), 800.0);
    }
}
