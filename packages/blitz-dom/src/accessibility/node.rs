use crate::{
    ElementData, Node, NodeData,
    accessibility::{FlatText, normalize},
    layout::replaced::is_inline_box_element,
};
use accesskit::Role;
use blitz_traits::NodeId;
use markup5ever::{LocalName, local_name};
use std::borrow::Cow;
use style::{
    properties::longhands::visibility,
    values::specified::box_::{DisplayInside, DisplayOutside},
};
use stylo_taffy::CopyStyleGetters as _;

impl Node {
    /// Builds an [accesskit::Node] with data from this instance alone.
    ///
    /// For an element, this sets the role, the HTML tag, the level, and the flags for
    /// `aria-hidden`, `aria-expanded`, `aria-disabled`, and `aria-selected`. For a text node, it
    /// sets the text as the value. The node without a parent, which is the document, gets the role
    /// [Role::Window].
    ///
    /// It does not set the properties which depend on other nodes or on the results above. In
    /// particular, the following are left to
    /// [TreeBuilder](super::tree_builder::TreeBuilder):
    ///
    /// - label, `labelled_by`, and the value of nodes with the role [Role::Label]
    /// - description and `described_by`
    /// - children
    pub(super) fn build_accesskit_node(&self) -> accesskit::Node {
        let mut accesskit_node = accesskit::Node::default();
        if self.parent.is_none() {
            accesskit_node.set_role(Role::Window)
        } else if let Some(element_data) = self.element_data() {
            accesskit_node.set_html_tag(element_data.name.local.as_str());

            // TODO: The roles of elements with strong native semantics cannot be overridden; see
            // https://www.w3.org/TR/wai-aria-1.2/#host_general_conflict.
            let role = element_role(element_data);
            accesskit_node.set_role(role);

            if let Some(aria_level) = element_data.attr(local_name!("aria-level"))
                && let Ok(aria_level) = aria_level.parse::<usize>()
                // Accesskit levels are 0-based, while ARIA levels are 1-based
                // See https://docs.rs/accesskit/latest/accesskit/struct.Node.html#method.level
                && let Some(aria_level) = aria_level.checked_sub(1)
            {
                accesskit_node.set_level(aria_level);
            } else if let Some(default_level) =
                default_level_for_tag(element_data.name.local.as_str())
            {
                accesskit_node.set_level(default_level);
            } else if let Some(default_level) = default_level_for_role(role) {
                accesskit_node.set_level(default_level);
            }

            // https://www.w3.org/TR/wai-aria-1.2/#tree_exclusion
            if element_data.attr(local_name!("aria-hidden")) == Some("true") {
                accesskit_node.set_hidden();
            }

            if let Some(aria_expanded) = element_data.attr(local_name!("aria-expanded"))
                && let Ok(aria_expanded) = aria_expanded.parse()
            {
                accesskit_node.set_expanded(aria_expanded);
            }

            if element_data.attr(local_name!("aria-disabled")) == Some("true") {
                accesskit_node.set_disabled();
            }

            if let Some(aria_selected) = element_data.attr(local_name!("aria-selected"))
                && let Ok(aria_selected) = aria_selected.parse()
            {
                accesskit_node.set_selected(aria_selected);
            }
        } else if self.is_text_node() {
            accesskit_node.set_role(Role::TextRun);
            accesskit_node.set_value(self.text_content());
        }

        accesskit_node
    }

    /// Sets the label in the given [accesskit::Node] to the accessible name computed from this
    /// node, its subtree and, for images, the enclosing `figure`. It does not take into account
    /// the nodes which refer to this node. [TreeBuilder](super::tree_builder::TreeBuilder) handles
    /// those first and falls back to this method.
    ///
    /// The role of `accesskit_node` must already be set. Call [Node::build_accesskit_node] first.
    ///
    /// <https://www.w3.org/TR/accname-1.2/#computation-steps>
    pub(super) fn set_accesskit_node_label(&self, accesskit_node: &mut accesskit::Node) {
        let Some(element_data) = self.element_data() else {
            return;
        };

        if let Some(aria_label) = element_data.attr(local_name!("aria-label"))
            && let Some(normalized_aria_label) = normalize(aria_label)
        {
            accesskit_node.set_label(normalized_aria_label);
            return;
        }

        if let Some(host_language_label) = self.host_language_label(element_data) {
            accesskit_node.set_label(host_language_label);
        } else if names_from_content(accesskit_node.role(), element_data) {
            let mut content = FlatText::default();
            self.write_text_alternative(&mut content, None);
            if !content.text.is_empty() {
                accesskit_node.set_label(content.text);
            }
        }
    }

    /// Sets the description in the given [accesskit::Node] from the attributes of this node. It
    /// does not take into account `aria-describedby`.
    /// [TreeBuilder](super::tree_builder::TreeBuilder) handles that first and falls back to this
    /// method.
    ///
    /// The label of `accesskit_node` must already be resolved, since `title` is only the
    /// description when it did not provide the name. See [Node::set_accesskit_node_label].
    ///
    /// <https://www.w3.org/TR/accname-1.2/#mapping_additional_nd_description>
    pub(super) fn set_accesskit_node_description(&self, accesskit_node: &mut accesskit::Node) {
        let Some(element_data) = self.element_data() else {
            return;
        };

        if let Some(aria_description) = element_data.attr(local_name!("aria-description"))
            && let Some(normalized_description) = normalize(aria_description)
        {
            accesskit_node.set_description(normalized_description);
            return;
        }

        if let Some(title_attr) = element_data.attr(local_name!("title"))
            && let Some(title) = normalize(title_attr)
            && (accesskit_node.label().is_some()
                || role_prohibits_naming(accesskit_node.role())
                || (accesskit_node.role() == Role::Image
                    && element_data
                        .attr(local_name!("alt"))
                        .is_some_and(|s| s.trim_ascii().is_empty())))
        {
            accesskit_node.set_description(title);
        }
    }

    /// The text alternative provided by the host language markup of this element itself.
    ///
    /// The `title` attribute is deliberately not used here. AccessKit expects a name which comes
    /// only from a tooltip in `Node::tooltip`, not `Node::label`, and that is not set yet. The
    /// `title` is used as the description; see [Node::set_accesskit_node_description]. Also not
    /// covered are `placeholder` and the strings which user agents supply for `input` elements of
    /// the submit, reset and image types lacking any other source. `area` elements are never
    /// exposed in the accessibility tree because the user agent stylesheet hides them.
    ///
    /// <https://www.w3.org/TR/html-aam-1.0/#accessible-name-and-description-computation>
    fn host_language_label<'a>(&'a self, element_data: &'a ElementData) -> Option<Cow<'a, str>> {
        match &*element_data.name.local {
            "img" => match element_data.attr(local_name!("alt")) {
                // An explicit `alt` attribute, even an empty one, rules out the other sources.
                Some(alt) => is_non_blank(Some(alt)).map(Cow::Borrowed),
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
            "input" => {
                let input_type = element_data.attr(local_name!("type")).unwrap_or_default();
                let source = if ["button", "submit", "reset"]
                    .iter()
                    .any(|button_type| input_type.eq_ignore_ascii_case(button_type))
                {
                    local_name!("value")
                } else if input_type.eq_ignore_ascii_case("image") {
                    local_name!("alt")
                } else {
                    return None;
                };
                is_non_blank(element_data.attr(source)).map(Cow::Borrowed)
            }
            "fieldset" => self
                .first_child_named(local_name!("legend"))?
                .subtree_text()
                .map(Cow::Owned),
            "table" => self
                .first_child_named(local_name!("caption"))?
                .subtree_text()
                .map(Cow::Owned),
            "summary" if self.is_first_summary_of_details() => self.subtree_text().map(Cow::Owned),
            _ => None,
        }
    }

    /// The text of the `figcaption` which names this element, if any. That is the first
    /// `figcaption` child of the closest `figure` ancestor, provided that this element and the
    /// `figcaption` are the only content of the `figure`. Any other element counts as content,
    /// even an empty one.
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
        let figcaption = figure.first_child_named(local_name!("figcaption"))?;
        if figure.has_content_other_than(self.id, figcaption.id, &path_to_figure) {
            return None;
        }
        figcaption.subtree_text()
    }

    /// The first child of this node which is an element with the given name.
    fn first_child_named(&self, name: LocalName) -> Option<&Node> {
        self.children
            .iter()
            .map(|&child_id| self.with(child_id))
            .find(|child| child.is_element_named(name.clone()))
    }

    /// Whether this is the first `summary` child of a `details` element.
    fn is_first_summary_of_details(&self) -> bool {
        self.parent
            .map(|parent_id| self.with(parent_id))
            .filter(|parent| parent.is_element_named(local_name!("details")))
            .and_then(|details| details.first_child_named(local_name!("summary")))
            .is_some_and(|summary| summary.id == self.id)
    }

    /// Whether the subtree of this node has non-whitespace text or elements besides `exempt` and
    /// `exempt_caption`. Elements in `ancestors_of_exempt` are searched rather than counted.
    fn has_content_other_than(
        &self,
        exempt: NodeId,
        exempt_caption: NodeId,
        ancestors_of_exempt: &[NodeId],
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

    /// Whether this node or any of its ancestors is hidden for naming; see
    /// [Node::is_hidden_for_naming].
    pub(super) fn is_in_hidden_subtree(&self) -> bool {
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

    /// Whether this node is hidden from the accessibility tree (see
    /// [Node::is_hidden_from_accessibility_tree]) or has `aria-hidden="true"`. Only this node is
    /// considered, not its ancestors.
    ///
    /// <https://www.w3.org/TR/accname-1.2/#comp_hidden_not_referenced>
    fn is_hidden_for_naming(&self) -> bool {
        self.is_hidden_from_accessibility_tree()
            || self.attr(local_name!("aria-hidden")) == Some("true")
    }

    /// Whether the style of this node excludes it from the accessibility tree, with
    /// `display: none` or `visibility: hidden`. This does not look at the ancestors.
    /// [TreeBuilder](super::tree_builder::TreeBuilder) excludes the descendants of such nodes.
    ///
    /// Nodes with `aria-hidden="true"` stay in the tree, flagged as hidden. See
    /// [Node::is_hidden_for_naming].
    ///
    /// <https://www.w3.org/TR/wai-aria-1.2/#tree_exclusion>
    pub(super) fn is_hidden_from_accessibility_tree(&self) -> bool {
        self.try_stylo_element_data()
            .as_ref()
            .and_then(|s| s.get())
            .is_some_and(|s| {
                s.styles.is_display_none()
                    || s.styles.primary().clone_visibility()
                        == visibility::computed_value::T::Hidden
            })
    }

    /// Whether a `<label>` can be associated with this element.
    ///
    /// Form-associated custom elements are also labelable per the HTML specification, but cannot
    /// be recognized here. `input type="hidden"` is not labelable either, but it is never exposed
    /// in the accessibility tree, so it needs no handling.
    ///
    /// <https://html.spec.whatwg.org/multipage/forms.html#category-label>
    pub(super) fn is_labelable(&self) -> bool {
        self.element_data().is_some_and(|element_data| {
            matches!(
                &*element_data.name.local,
                "button" | "input" | "meter" | "output" | "progress" | "select" | "textarea"
            )
        })
    }

    /// The first labelable element in the subtree of this node, in tree order.
    pub(super) fn first_labelable_descendant(&self) -> Option<NodeId> {
        self.children.iter().find_map(|&child_id| {
            let child = self.with(child_id);
            if child.is_labelable() {
                Some(child_id)
            } else {
                child.first_labelable_descendant()
            }
        })
    }

    /// Writes the text alternative of this node, leaving out the content of the node `skip`.
    ///
    /// Hidden nodes are left out, unless this node or one of its ancestors is hidden. Then the
    /// whole subtree is included.
    ///
    /// <https://www.w3.org/TR/accname-1.2/#comp_hidden_not_referenced>
    pub(super) fn write_text_alternative(&self, out: &mut FlatText, skip: Option<NodeId>) {
        let context = TextContext {
            skip,
            include_hidden: self.is_in_hidden_subtree(),
        };
        self.write_flat_text_content(out, context);
    }

    /// Writes the text alternative of this node, following the steps of
    /// <https://www.w3.org/TR/accname-1.2/#computation-steps> which apply to a node reached by
    /// `aria-labelledby` or by name from content.
    ///
    /// `context` says which node to leave out and whether hidden nodes count. Where the
    /// specification says nothing about it, words are separated by a space around block-level
    /// boxes, atomic inlines, and line breaks, but not around plain inline content.
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
        if let Some(aria_label) = is_non_blank(element_data.attr(local_name!("aria-label"))) {
            out.push(aria_label);
            return;
        }
        if self.is_element_named(local_name!("br")) {
            out.break_word();
            return;
        }
        if self.is_element_named(local_name!("img")) {
            if let Some(alt) = is_non_blank(element_data.attr(local_name!("alt"))) {
                out.push(alt);
            }
            return;
        }

        // Generated content is part of the content of the element: `::before` comes before the
        // children and `::after` after them. The accname spec joins it to the content of the
        // element without a space, and so do we, unless the pseudo-element is itself block-level
        // or an atomic inline. Those are separated like any other child; the spec leaves this
        // open (https://github.com/w3c/accname/issues/225).
        let before = self.before();
        let after = self.after();
        for &child_id in before
            .iter()
            .chain(self.children.iter())
            .chain(after.iter())
        {
            let child = self.with(child_id);
            let separate_child_from_siblings = child.should_separate_from_siblings();
            if separate_child_from_siblings {
                out.break_word();
            }
            child.write_flat_text_content(out, context);
            if separate_child_from_siblings {
                out.break_word();
            }
        }
    }

    /// Whether the text of this node is separated by a space from the text around it. This is the
    /// case for visible block-level boxes, atomic inlines, and inline box elements. The
    /// specification leaves this open: <https://github.com/w3c/accname/issues/225>
    fn should_separate_from_siblings(&self) -> bool {
        (self.is_block_or_atomic_inline() || self.is_inline_box_element())
            && !self.is_hidden_for_naming()
    }

    /// Whether this node generates a block-level box, a table caption or internal table box, or
    /// an atomic inline such as `inline-block` or `inline-flex`. Plain inline boxes, and nodes
    /// with `display: none` or `display: contents`, do not.
    fn is_block_or_atomic_inline(&self) -> bool {
        self.display_style()
            .is_some_and(|display| match display.outside() {
                DisplayOutside::Block
                | DisplayOutside::TableCaption
                | DisplayOutside::InternalTable => true,
                DisplayOutside::Inline => display.inside() != DisplayInside::Flow,
                DisplayOutside::None => false,
            })
    }

    /// Whether this is an element which is laid out as an inline box: a replaced element such as
    /// `img`, an `input`, a `textarea`, or a `button`.
    fn is_inline_box_element(&self) -> bool {
        self.element_data()
            .is_some_and(|element_data| is_inline_box_element(&element_data.name.local))
    }

    /// Writes the value of this element, if it is a control the user can adjust, in place of
    /// its content. Returns whether it did.
    ///
    /// Password inputs are deliberately not included, so as not to expose their value.
    ///
    /// <https://www.w3.org/TR/accname-1.2/#comp_embedded_control>
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
                for (index, option) in chosen.into_iter().enumerate() {
                    // The specification only describes a single chosen option. Several are
                    // separated by a space, as browsers do.
                    if index > 0 {
                        out.break_word();
                    }
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
    fn option_descendants(&self) -> Vec<&Node> {
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

    /// The text alternative of the subtree of this node, if it is not empty.
    ///
    /// Unlike [Node::write_text_alternative], hidden nodes are always left out, including this
    /// node itself.
    fn subtree_text(&self) -> Option<String> {
        let mut text = FlatText::default();
        self.write_flat_text_content(&mut text, TextContext::default());
        (!text.text.is_empty()).then_some(text.text)
    }

    /// Returns whether this instance has the given [LocalName] as HTML tag.
    pub(super) fn is_element_named(&self, name: LocalName) -> bool {
        self.element_data()
            .is_some_and(|element_data| element_data.name.local == name)
    }
}

/// How the text alternative of a subtree is written.
#[derive(Clone, Copy, Default)]
struct TextContext {
    /// A node whose content is left out.
    skip: Option<NodeId>,
    /// Whether hidden nodes are included. This is the case when the root of the computation is
    /// itself hidden or inside a hidden subtree, and for the chosen options of a `select`.
    include_hidden: bool,
}

/// The given text, if it contains something other than ASCII whitespace.
fn is_non_blank(text: Option<&str>) -> Option<&str> {
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

/// The role which the value of a `role` attribute names, if it is one of the supported roles.
/// Other values give `None`, and the element then has its native role.
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

/// The native role of an element as given by HTML-AAM, or `None` if there is no mapping.
///
/// <https://www.w3.org/TR/html-aam-1.0/>
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
        "span" => Some(Role::GenericContainer),

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

/// The default level of a heading element. AccessKit levels are zero-based, while ARIA levels are
/// one-based.
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

/// The implicit level of an element with the given role, if its role has one. AccessKit levels
/// are zero-based, while ARIA levels are one-based.
fn default_level_for_role(role: Role) -> Option<usize> {
    match role {
        // ARIA specifies an implicit level of 2 for the heading role. Accesskit levels are 0-based,
        // so this turns into level 1
        Role::Heading => Some(1),
        _ => None,
    }
}

/// Whether the name of an element with the given role is computed from its content.
///
/// <https://www.w3.org/TR/wai-aria-1.2/#namefromcontent>
///
/// HTML-AAM names the native `tr`, `td` and `th` elements from their `title` only, so they are
/// exceptions unless a `role` attribute gives them a role.
fn names_from_content(role: Role, element_data: &ElementData) -> bool {
    let is_native_table_part = matches!(&*element_data.name.local, "tr" | "td" | "th")
        && element_data
            .attr(local_name!("role"))
            .and_then(role_from_name)
            .is_none();
    !is_native_table_part
        && matches!(
            role,
            Role::Button
                | Role::Cell
                | Role::CheckBox
                | Role::ColumnHeader
                | Role::DefaultButton
                | Role::GridCell
                | Role::Heading
                | Role::Link
                | Role::ListBoxOption
                | Role::MenuItem
                | Role::MenuItemCheckBox
                | Role::MenuItemRadio
                | Role::MenuListOption
                | Role::RadioButton
                | Role::Row
                | Role::RowHeader
                | Role::Switch
                | Role::Tab
                | Role::Tooltip
                | Role::TreeItem
        )
}

/// Whether an element with the given role cannot be named.
///
/// <https://www.w3.org/TR/wai-aria-1.2/#namefromprohibited>
fn role_prohibits_naming(role: Role) -> bool {
    matches!(
        role,
        Role::Caption
            | Role::Code
            | Role::ContentDeletion
            | Role::ContentInsertion
            | Role::Emphasis
            | Role::GenericContainer
            | Role::Paragraph
            | Role::Strong
    )
}
