use crate::{
    BaseDocument, ElementData, Node as BlitzDomNode, NodeData, NodeId as BlitzDomNodeId, local_name,
};
use accesskit::{Node as AccessKitNode, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use markup5ever::LocalName;
use std::borrow::Cow;
use style::properties::longhands::visibility;
use stylo_taffy::CopyStyleGetters;

/// An element together with the nodes which provide its label.
struct LabelSources {
    labelled: BlitzDomNodeId,
    labels: Vec<BlitzDomNodeId>,
    /// Whether `labels` are `<label>` elements, as opposed to targets of `aria-labelledby`.
    from_label_elements: bool,
}

/// How the text alternative of a subtree is written.
#[derive(Clone, Copy, Default)]
struct TextContext {
    /// A node whose content is left out.
    skip: Option<BlitzDomNodeId>,
    /// Whether hidden nodes are included. This is the case when the root of the computation is
    /// itself hidden.
    include_hidden: bool,
}

impl BaseDocument {
    pub fn build_accessibility_tree(&self) -> TreeUpdate {
        let mut nodes = std::collections::HashMap::new();
        let mut window = AccessKitNode::new(Role::Window);
        let mut hidden_nodes = std::collections::HashSet::new();
        let mut labelled_by_nodes: Vec<LabelSources> = Vec::new();
        let mut described_by_nodes = Vec::new();

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

            if let Some(id_refs) = node.attr(local_name!("aria-labelledby")) {
                labelled_by_nodes.push(LabelSources {
                    labelled: node_id,
                    labels: self.referenced_dom_ids_to_node_ids(id_refs).collect(),
                    from_label_elements: false,
                });
            } else if node.is_element_named(local_name!("label"))
                && let Some(target) = self.label_control(node)
                // aria-labelledby on the target takes precedence over <label>.
                && self.nodes[target].attr(local_name!("aria-labelledby")).is_none()
                // So does aria-label.
                && self.nodes[target]
                    .attr(local_name!("aria-label"))
                    .is_none_or(|aria_label| aria_label.trim_ascii().is_empty())
            {
                match labelled_by_nodes
                    .iter_mut()
                    .find(|sources| sources.labelled == target)
                {
                    Some(sources) => sources.labels.push(node_id),
                    None => labelled_by_nodes.push(LabelSources {
                        labelled: target,
                        labels: vec![node_id],
                        from_label_elements: true,
                    }),
                }
            }
            if node.attr(local_name!("aria-describedby")).is_some() {
                described_by_nodes.push(node_id);
            }
            nodes.insert(node_id, (id, builder));
        });

        let mut label = FlatText::default();
        for LabelSources {
            labelled: node_id,
            labels,
            from_label_elements,
        } in labelled_by_nodes
        {
            let Some((id, mut builder)) = nodes.remove(&node_id) else {
                continue;
            };
            let mut has_target = false;
            label.clear();
            for target in labels {
                if !has_target {
                    builder.clear_labelled_by();
                    has_target = true;
                }
                if target == node_id || nodes.contains_key(&target) {
                    builder.push_labelled_by(NodeId(target.as_u64()));
                }
                self.nodes[target].write_text_alternative(
                    &mut label,
                    // A <label> does not contribute the content of the element it labels.
                    from_label_elements.then_some(node_id),
                );
                label.break_word();
            }
            if !label.text.is_empty() {
                builder.set_label(label.text.as_str());
            }
            nodes.insert(node_id, (id, builder));
        }

        for node_id in described_by_nodes {
            let Some(id_refs) = self.nodes[node_id].attr(local_name!("aria-describedby")) else {
                continue;
            };
            let Some((id, mut builder)) = nodes.remove(&node_id) else {
                continue;
            };
            for target in self.referenced_dom_ids_to_node_ids(id_refs) {
                if target == node_id || nodes.contains_key(&target) {
                    builder.push_described_by(NodeId(target.as_u64()));
                }
            }
            nodes.insert(node_id, (id, builder));
        }

        let mut nodes: Vec<_> = nodes
            .into_iter()
            .map(|(_, (id, node))| (id, node))
            .collect();
        nodes.push((NodeId(u64::MAX), window));

        let tree = TreeInfo::new(NodeId(u64::MAX));
        TreeUpdate {
            tree_id: TreeId::ROOT,
            nodes,
            tree: Some(tree),
            focus: NodeId(self.focus_node_id.map(|id| id.as_u64()).unwrap_or(u64::MAX)),
        }
    }

    /// The element labelled by the given `<label>` element, if any.
    ///
    /// https://html.spec.whatwg.org/multipage/forms.html#dom-label-control
    fn label_control(&self, label: &BlitzDomNode) -> Option<BlitzDomNodeId> {
        match label.attr(local_name!("for")) {
            // The spec restricts which elements can be labelled. If the first element with the id
            // is not labelable, no later element with the same id is used and the descendants of
            // the label are not considered either.
            Some(for_id) => self
                .nodes_to_id
                .get(for_id)
                .and_then(|targets| targets.first().copied())
                .filter(|&target| self.nodes[target].is_labelable()),
            None => label.first_labelable_descendant(),
        }
    }

    fn referenced_dom_ids_to_node_ids<'a>(
        &'a self,
        nodes_reference: &'a str,
    ) -> impl Iterator<Item = BlitzDomNodeId> + 'a {
        nodes_reference
            .split_ascii_whitespace()
            .filter_map(|dom_id| self.nodes_to_id.get(dom_id)?.first().copied())
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
            // TODO: The roles of elements with strong native semantics cannot be overridden; see
            // https://www.w3.org/TR/wai-aria-1.2/#host_general_conflict.
            let role = element_role(element_data);

            builder.set_role(role);

            // https://www.w3.org/TR/wai-aria-1.2/#tree_exclusion
            if element_data.attr(local_name!("aria-hidden")) == Some("true") {
                builder.set_hidden();
            }

            if let Some(aria_expanded) = element_data.attr(local_name!("aria-expanded"))
                && let Ok(aria_expanded) = aria_expanded.parse()
            {
                builder.set_expanded(aria_expanded);
            }

            if element_data.attr(local_name!("aria-disabled")) == Some("true") {
                builder.set_disabled();
            }

            if let Some(aria_selected) = element_data.attr(local_name!("aria-selected"))
                && let Ok(aria_selected) = aria_selected.parse()
            {
                builder.set_selected(aria_selected);
            }

            if let Some(aria_label) = element_data
                .attr(local_name!("aria-label"))
                .filter(|aria_label| !aria_label.trim_ascii().is_empty())
            {
                builder.set_label(aria_label);
            } else if let Some(host_language_label) = node.host_language_label(element_data) {
                builder.set_label(host_language_label);
            }
            if let Some(aria_description) = element_data.attr(local_name!("aria-description")) {
                builder.set_description(aria_description);
            }
            if let Some(aria_level) = element_data.attr(local_name!("aria-level"))
                && let Ok(aria_level) = aria_level.parse::<usize>()
                // Accesskit levels are 0-based, while ARIA levels are 1-based
                // See https://docs.rs/accesskit/latest/accesskit/struct.Node.html#method.level
                && let Some(aria_level) = aria_level.checked_sub(1)
            {
                builder.set_level(aria_level);
            } else if let Some(default_level) = default_level_for_tag(&name) {
                builder.set_level(default_level);
            } else if let Some(default_level) = default_level_for_role(role) {
                builder.set_level(default_level);
            }

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

    // https://www.w3.org/TR/accname-1.2/#comp_hidden_not_referenced
    fn is_hidden_for_naming(&self) -> bool {
        self.is_hidden_from_accessibility_tree()
            || self.attr(local_name!("aria-hidden")) == Some("true")
    }

    fn is_in_hidden_subtree(&self) -> bool {
        let mut node = self;
        loop {
            if node.is_hidden_for_naming() {
                return true;
            }
            match node.parent {
                Some(parent_id) => node = self.with(parent_id),
                None => return false,
            }
        }
    }

    /// Writes the text alternative of this node, leaving out the content of the node `skip`.
    ///
    /// If this node is itself hidden, as is possible when it is referenced by `aria-labelledby`,
    /// its whole subtree is included, hidden parts and all.
    fn write_text_alternative(&self, out: &mut FlatText, skip: Option<BlitzDomNodeId>) {
        let context = TextContext {
            skip,
            include_hidden: self.is_in_hidden_subtree(),
        };
        self.write_flat_text_content(out, context);
    }

    /// Writes the text alternative of this node, following the steps of
    /// https://www.w3.org/TR/accname-1.2/#computation-steps which apply to a node reached by
    /// `aria-labelledby` or by name from content.
    fn write_flat_text_content(&self, out: &mut FlatText, context: TextContext) {
        if context.skip == Some(self.id) {
            return;
        }
        let element_data = match &self.data {
            NodeData::Text(data) => {
                out.push(&data.content);
                return;
            }
            NodeData::Element(element_data) | NodeData::AnonymousBlock(element_data) => {
                element_data
            }
            _ => return,
        };
        if !context.include_hidden && self.is_hidden_for_naming() {
            return;
        }
        if self.write_embedded_control_text(element_data, out) {
            return;
        }
        if let Some(aria_label) = non_blank(element_data.attr(local_name!("aria-label"))) {
            out.push(aria_label);
            return;
        }
        if element_data.name.local == local_name!("img") {
            if let Some(alt) = non_blank(element_data.attr(local_name!("alt"))) {
                out.push(alt);
            }
            return;
        }

        // Generated content is part of the content of the element: `::before` comes before the
        // children and `::after` after them, without a space.
        if let Some(before_id) = self.before() {
            self.with(before_id).write_flat_text_content(out, context);
        }
        for &child_id in &self.children {
            self.with(child_id).write_flat_text_content(out, context);
        }
        if let Some(after_id) = self.after() {
            self.with(after_id).write_flat_text_content(out, context);
        }
    }

    /// Writes the value of this element, if it is a control the user can adjust, in place of
    /// its content. Returns whether it did.
    ///
    /// Password inputs are deliberately not included, so as not to expose their value.
    ///
    /// https://www.w3.org/TR/accname-1.2/#comp_embedded_control
    fn write_embedded_control_text(&self, element_data: &ElementData, out: &mut FlatText) -> bool {
        let role = element_role(element_data);
        match role {
            Role::TextInput
            | Role::MultilineTextInput
            | Role::EmailInput
            | Role::SearchInput
            | Role::PhoneNumberInput => {
                // Only native controls have a value distinct from their content.
                let Some(text_input) = element_data.text_input_data() else {
                    return false;
                };
                out.push(&text_input.editor.text().to_string());
                true
            }
            Role::Slider | Role::SpinButton | Role::NumberInput => {
                let value = element_data
                    .attr(local_name!("aria-valuetext"))
                    .or_else(|| element_data.attr(local_name!("aria-valuenow")))
                    .or_else(|| element_data.attr(local_name!("value")));
                if let Some(value) = value {
                    out.push(value);
                }
                true
            }
            Role::ComboBox | Role::ListBox => {
                let options = self.option_descendants();
                let mut chosen: Vec<_> = options
                    .iter()
                    .filter(|option| {
                        option.attr(local_name!("selected")).is_some()
                            || option.attr(local_name!("aria-selected")) == Some("true")
                    })
                    .collect();
                // A drop-down `select` always has a chosen option, which is the first one unless
                // another is marked as selected.
                let is_single_select = element_data.name.local == local_name!("select")
                    && element_data.attr(local_name!("multiple")).is_none();
                if chosen.is_empty() && is_single_select {
                    chosen.extend(options.first());
                }
                for option in chosen {
                    // The options of a collapsed `select` are not rendered, but the chosen one is
                    // still the value of the control.
                    let context = TextContext {
                        skip: None,
                        include_hidden: true,
                    };
                    option.write_flat_text_content(out, context);
                }
                true
            }
            _ => false,
        }
    }

    /// The `option` elements in the subtree of this node, in tree order.
    fn option_descendants(&self) -> Vec<&BlitzDomNode> {
        let mut options = Vec::new();
        let mut pending: Vec<_> = self.children.iter().rev().copied().collect();
        while let Some(node_id) = pending.pop() {
            let node = self.with(node_id);
            if node
                .element_data()
                .is_some_and(|element_data| element_role(element_data) == Role::ListBoxOption)
            {
                options.push(node);
            } else {
                pending.extend(node.children.iter().rev().copied());
            }
        }
        options
    }

    /// The text alternative provided by the host language markup of this element itself, such as
    /// the `alt` text of an `img`.
    ///
    /// https://www.w3.org/TR/html-aam-1.0/#img-element-accessible-name-computation
    fn host_language_label<'a>(&'a self, element_data: &'a ElementData) -> Option<Cow<'a, str>> {
        match &*element_data.name.local {
            "img" => match element_data.attr(local_name!("alt")) {
                // An explicit `alt` attribute, even an empty one, rules out the other sources.
                Some(alt) => non_blank(Some(alt)).map(Cow::Borrowed),
                // Per HTML-AAM, the order of sources is `alt`, then `title`, then `figcaption`.
                // `title` is deliberately not exposed as the label here: AccessKit expects a name
                // which comes only from a tooltip in `Node::tooltip`, not `Node::label`. Its
                // presence still has to be checked, because it ranks above `figcaption`: with a
                // `title`, the `figcaption` is not the name, so it must not become the label.
                None if element_data.attr(local_name!("title")).is_none() => {
                    self.figcaption_text().map(Cow::Owned)
                }
                None => None,
            },
            _ => None,
        }
    }

    /// The text of the `figcaption` of the closest `figure` ancestor of this element, provided
    /// that this element and the `figcaption` are the only non-whitespace content of that `figure`.
    fn figcaption_text(&self) -> Option<String> {
        let mut path_to_figure = Vec::new();
        let mut current = self.parent;
        let figure = loop {
            let ancestor = self.with(current?);
            if ancestor.is_element_named(local_name!("figure")) {
                break ancestor;
            }
            path_to_figure.push(ancestor.id);
            current = ancestor.parent;
        };
        let figcaption = figure
            .children
            .iter()
            .map(|&child_id| self.with(child_id))
            .find(|child| child.is_element_named(local_name!("figcaption")))?;
        if figure.has_content_other_than(self.id, figcaption.id, &path_to_figure) {
            return None;
        }

        let mut text = FlatText::default();
        figcaption.write_flat_text_content(&mut text, TextContext::default());
        (!text.text.is_empty()).then_some(text.text)
    }

    /// Whether the subtree of this node has non-whitespace text or elements besides `exempt` and
    /// `exempt_caption`. Elements in `ancestors_of_exempt` are searched rather than counted.
    fn has_content_other_than(
        &self,
        exempt: BlitzDomNodeId,
        exempt_caption: BlitzDomNodeId,
        ancestors_of_exempt: &[BlitzDomNodeId],
    ) -> bool {
        self.children.iter().any(|&child_id| {
            let child = self.with(child_id);
            if child_id == exempt || child_id == exempt_caption {
                false
            } else if ancestors_of_exempt.contains(&child_id) {
                child.has_content_other_than(exempt, exempt_caption, ancestors_of_exempt)
            } else {
                match &child.data {
                    NodeData::Text(data) => !data.content.trim_ascii().is_empty(),
                    NodeData::Element(..) => true,
                    _ => false,
                }
            }
        })
    }

    fn is_element_named(&self, name: LocalName) -> bool {
        self.element_data()
            .is_some_and(|element_data| element_data.name.local == name)
    }

    /// Whether a `<label>` can be associated with this element.
    ///
    /// Form-associated custom elements are also labelable per the HTML specification, but cannot
    /// be recognized here. `input type="hidden"` is not labelable either, but it is never exposed
    /// in the accessibility tree, so it needs no handling.
    ///
    /// https://html.spec.whatwg.org/multipage/forms.html#category-label
    fn is_labelable(&self) -> bool {
        self.element_data().is_some_and(|element_data| {
            matches!(
                &*element_data.name.local,
                "button" | "input" | "meter" | "output" | "progress" | "select" | "textarea"
            )
        })
    }

    /// The first labelable element in the subtree of this node, in tree order.
    fn first_labelable_descendant(&self) -> Option<BlitzDomNodeId> {
        self.children.iter().find_map(|&child_id| {
            let child = self.with(child_id);
            if child.is_labelable() {
                Some(child_id)
            } else {
                child.first_labelable_descendant()
            }
        })
    }
}

fn non_blank(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.trim_ascii().is_empty())
}

/// The role of an element: its `role` attribute if it has a valid one, or else its native role.
fn element_role(element_data: &ElementData) -> Role {
    element_data
        .attr(local_name!("role"))
        .and_then(role_from_name)
        .or_else(|| role_from_element_data(element_data))
        .unwrap_or(Role::Unknown)
}

#[derive(Default)]
struct FlatText {
    text: String,
    pending_space: bool,
}

impl FlatText {
    fn push(&mut self, text: &str) {
        for (i, word) in text.split(|c: char| c.is_ascii_whitespace()).enumerate() {
            self.pending_space |= i > 0;
            if word.is_empty() {
                continue;
            }
            if self.pending_space && !self.text.is_empty() {
                self.text.push(' ');
            }
            self.pending_space = false;
            self.text.push_str(word);
        }
    }

    fn break_word(&mut self) {
        self.pending_space = true;
    }

    fn clear(&mut self) {
        self.text.clear();
        self.pending_space = false;
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

fn default_level_for_tag(name: &str) -> Option<usize> {
    // Accesskit levels are 0-based, while ARIA levels are 1-based
    // See https://docs.rs/accesskit/latest/accesskit/struct.Node.html#method.level
    match name {
        "h1" => Some(0),
        "h2" => Some(1),
        "h3" => Some(2),
        "h4" => Some(3),
        "h5" => Some(4),
        "h6" => Some(5),
        _ => None,
    }
}

fn default_level_for_role(role: Role) -> Option<usize> {
    match role {
        // ARIA specifies an implicit level of 2 for the heading role. Accesskit levels are 0-based,
        // so this turns into level 1
        Role::Heading => Some(1),
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
