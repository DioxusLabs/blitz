use std::collections::HashSet;

use crate::net::{CssHandler, ImageHandler};
use crate::node::NodeSpecificData;
use crate::util::ImageType;
use crate::{Attribute, BaseDocument, ElementNodeData, NodeData, QualName, local_name, ns};
use blitz_traits::net::Request;
use style::invalidation::element::restyle_hints::RestyleHint;

pub enum AppendTextErr {
    /// The node is not a text node
    NotTextNode,
}

pub struct DocumentMutator<'doc> {
    /// Document is public as an escape hatch, but users of this API should ideally avoid using it
    /// and prefer exposing additional functionality in DocumentMutator.
    pub doc: &'doc mut BaseDocument,

    // Tracked nodes for deferred processing when mutations have completed
    style_nodes: HashSet<usize>,
    form_nodes: HashSet<usize>,
    /// The (latest) node which has been mounted in and had autofocus=true, if any
    #[cfg(feature = "autofocus")]
    node_to_autofocus: Option<usize>,
}

impl Drop for DocumentMutator<'_> {
    fn drop(&mut self) {
        self.flush(); // Defined at bottom of file
    }
}

impl DocumentMutator<'_> {
    pub fn new<'doc>(doc: &'doc mut BaseDocument) -> DocumentMutator<'doc> {
        DocumentMutator {
            doc,
            style_nodes: HashSet::new(),
            form_nodes: HashSet::new(),
            #[cfg(feature = "autofocus")]
            node_to_autofocus: None,
        }
    }

    pub fn node_has_parent(&self, node_id: usize) -> bool {
        self.doc.nodes[node_id].parent.is_some()
    }

    pub fn previous_sibling_id(&self, node_id: usize) -> Option<usize> {
        self.doc.nodes[node_id].backward(1).map(|node| node.id)
    }

    pub fn next_sibling_id(&self, node_id: usize) -> Option<usize> {
        self.doc.nodes[node_id].forward(1).map(|node| node.id)
    }

    pub fn last_child_id(&self, node_id: usize) -> Option<usize> {
        self.doc.nodes[node_id].children.last().copied()
    }

    pub fn element_name(&self, node_id: usize) -> Option<&QualName> {
        self.doc.nodes[node_id].element_data().map(|el| &el.name)
    }

    pub fn node_at_path(&self, start_node_id: usize, path: &[u8]) -> usize {
        let mut current = &self.doc.nodes[start_node_id];
        for i in path {
            let new_id = current.children[*i as usize];
            current = &self.doc.nodes[new_id];
        }
        current.id
    }

    pub fn create_comment_node(&mut self) -> usize {
        self.doc.create_node(NodeData::Comment)
    }

    pub fn create_text_node(&mut self, text: &str) -> usize {
        self.doc.create_text_node(text)
    }

    /// Remove all of the children from old_parent_id and append them to new_parent_id
    pub fn reparent_children(&mut self, old_parent_id: usize, new_parent_id: usize) {
        let child_ids = std::mem::take(&mut self.doc.nodes[old_parent_id].children);
        self.maybe_push_style_node(old_parent_id);
        self.append_children(new_parent_id, &child_ids);
    }

    pub fn append_children(&mut self, parent_id: usize, child_ids: &[usize]) {
        for child_id in child_ids.iter().copied() {
            self.doc.nodes[parent_id].children.push(child_id);
            let old_parent = self.doc.nodes[child_id].parent.replace(parent_id);
            if let Some(old_parent_id) = old_parent {
                self.doc.nodes[old_parent_id]
                    .children
                    .retain(|id| *id != child_id);
                self.maybe_push_style_node(old_parent);
            }
        }

        self.maybe_push_style_node(parent_id);
    }

    pub fn replace_node_with(&mut self, anchor_node_id: usize, new_node_ids: &[usize]) {
        self.maybe_push_parent_style_node(anchor_node_id);
        self.doc.insert_before(anchor_node_id, new_node_ids);
        self.doc.remove_node(anchor_node_id);
    }

    pub fn replace_placeholder_with_nodes(
        &mut self,
        anchor_node_id: usize,
        new_node_ids: &[usize],
    ) {
        self.maybe_push_parent_style_node(anchor_node_id);
        self.doc.insert_before(anchor_node_id, new_node_ids);
        self.doc.remove_node(anchor_node_id);
    }

    pub fn remove_node(&mut self, node_id: usize) {
        self.doc.remove_node(node_id);
    }

    pub fn insert_nodes_after(&mut self, anchor_node_id: usize, new_node_ids: &[usize]) {
        let next_sibling_id = self
            .doc
            .get_node(anchor_node_id)
            .unwrap()
            .forward(1)
            .map(|node| node.id);

        match next_sibling_id {
            Some(anchor_node_id) => {
                self.doc.insert_before(anchor_node_id, new_node_ids);
            }
            None => self.doc.append(anchor_node_id, new_node_ids),
        }

        self.maybe_push_parent_style_node(anchor_node_id);
    }

    pub fn insert_nodes_before(&mut self, anchor_node_id: usize, new_node_ids: &[usize]) {
        self.doc.insert_before(anchor_node_id, new_node_ids);
        self.maybe_push_parent_style_node(anchor_node_id);
    }

    pub fn remove_node_if_unparented(&mut self, node_id: usize) {
        if let Some(node) = self.doc.get_node(node_id) {
            if node.parent.is_none() {
                self.doc.remove_and_drop_node(node_id);
            }
        }
    }

    pub fn append_text_to_node(&mut self, node_id: usize, text: &str) -> Result<(), AppendTextErr> {
        match self.doc.nodes[node_id].text_data_mut() {
            Some(data) => {
                data.content += text;
                Ok(())
            }
            None => Err(AppendTextErr::NotTextNode),
        }
    }

    pub fn set_node_text(&mut self, node_id: usize, value: &str) {
        let node = self.doc.get_node_mut(node_id).unwrap();

        let text = match node.data {
            NodeData::Text(ref mut text) => text,
            // TODO: otherwise this is basically element.textContent which is a bit different - need to parse as html
            _ => return,
        };

        let changed = text.content != value;
        if changed {
            text.content.clear();
            text.content.push_str(value);
            let parent = node.parent;
            self.maybe_push_style_node(parent);
        }
    }

    pub fn deep_clone_node(&mut self, node_id: usize) -> usize {
        // TODO: react to mutations
        let clone_id = self.doc.deep_clone_node(node_id);

        #[cfg(feature = "autofocus")]
        process_cloned_node(self.doc, &mut self.node_to_autofocus, clone_id);

        clone_id
    }

    pub fn create_element(&mut self, name: QualName, attrs: Vec<Attribute>) -> usize {
        let mut data = ElementNodeData::new(name, attrs);
        data.flush_style_attribute(self.doc.guard(), self.doc.base_url.clone());

        let id = self.doc.create_node(NodeData::Element(data));
        let node = self.doc.get_node(id).unwrap();

        // Initialise style data
        *node.stylo_element_data.borrow_mut() = Some(Default::default());

        // If the node has an "id" attribute, store it in the ID map.
        if let Some(id_attr) = node.attr(local_name!("id")) {
            self.doc.nodes_to_id.insert(id_attr.to_string(), id);
        }

        // Custom post-processing by element tag name
        let node = &self.doc.nodes[id];
        let tag = node.element_data().unwrap().name.local.as_ref();
        match tag {
            "link" => self.load_linked_stylesheet(id),
            "img" => self.load_image(id),
            "style" => {
                self.style_nodes.insert(id);
            }
            "button" | "fieldset" | "input" | "select" | "textarea" | "object" | "output" => {
                self.process_button_input(id);
                self.form_nodes.insert(id);
            }
            _ => {}
        }

        id
    }

    pub fn add_attrs_if_missing(&mut self, node_id: usize, attrs: Vec<Attribute>) {
        let node = &mut self.doc.nodes[node_id];
        let element_data = node.element_data_mut().expect("Not an element");

        let existing_names = element_data
            .attrs
            .iter()
            .map(|e| e.name.clone())
            .collect::<HashSet<_>>();

        for attr in attrs
            .into_iter()
            .filter(|attr| !existing_names.contains(&attr.name))
        {
            self.set_attribute(node_id, attr.name, &attr.value);
        }
    }

    pub fn set_attribute(&mut self, node_id: usize, name: QualName, value: &str) {
        self.doc.snapshot_node(node_id);

        let node = &mut self.doc.nodes[node_id];
        if let Some(data) = &mut *node.stylo_element_data.borrow_mut() {
            data.hint |= RestyleHint::restyle_subtree();
        }

        let NodeData::Element(ref mut element) = node.data else {
            return;
        };

        let attr = name.local.as_ref();
        let load_image = element.name.local == local_name!("img") && attr == "src";

        if element.name.local == local_name!("input") && attr == "checked" {
            set_input_checked_state(element, value.to_string());
        } else {
            if attr == "value" {
                // Update text input value
                if let Some(input_data) = element.text_input_data_mut() {
                    input_data.set_text(&mut self.doc.font_ctx, &mut self.doc.layout_ctx, value);
                }
            }

            let existing_attr = element.attrs.iter_mut().find(|a| a.name == name);
            if let Some(existing_attr) = existing_attr {
                existing_attr.value.clear();
                existing_attr.value.push_str(value);
            } else {
                element.attrs.push(Attribute {
                    name: name.clone(),
                    value: value.to_string(),
                });
            }

            if attr == "style" {
                element.flush_style_attribute(&self.doc.guard, self.doc.base_url.clone());
            }
        }

        if load_image {
            self.load_image(node_id);
        }
    }

    pub fn clear_attribute(&mut self, node_id: usize, name: QualName) {
        self.doc.snapshot_node(node_id);

        let node = &mut self.doc.nodes[node_id];

        let stylo_element_data = &mut *node.stylo_element_data.borrow_mut();
        if let Some(data) = stylo_element_data {
            data.hint |= RestyleHint::restyle_subtree();
        }

        if let NodeData::Element(ref mut element) = node.data {
            // Update text input value
            if name.local == local_name!("value") {
                if let Some(input_data) = element.text_input_data_mut() {
                    input_data.set_text(&mut self.doc.font_ctx, &mut self.doc.layout_ctx, "");
                }
            }

            // FIXME: check namespace
            element.attrs.retain(|attr| attr.name.local != name.local);
        }
    }
}

impl<'doc> DocumentMutator<'doc> {
    pub fn flush(&mut self) {
        // Add/Update inline stylesheets (<style> elements)
        for id in self.style_nodes.drain() {
            self.doc.process_style_element(id);
        }

        for id in self.form_nodes.drain() {
            self.doc.reset_form_owner(id);
        }

        #[cfg(feature = "autofocus")]
        if let Some(node_id) = self.node_to_autofocus.take() {
            if self.doc.get_node(node_id).is_some() {
                self.doc.set_focus_to(node_id);
            }
        }
    }

    fn is_style_node(&self, node_id: usize) -> bool {
        self.doc.nodes[node_id]
            .data
            .is_element_with_tag_name(&local_name!("style"))
    }

    fn maybe_push_style_node(&mut self, node_id: impl Into<Option<usize>>) {
        if let Some(node_id) = node_id.into() {
            if self.is_style_node(node_id) {
                self.style_nodes.insert(node_id);
            }
        }
    }

    #[track_caller]
    fn maybe_push_parent_style_node(&mut self, node_id: usize) {
        let parent_id = self.doc.get_node(node_id).unwrap().parent;
        self.maybe_push_style_node(parent_id);
    }

    fn load_linked_stylesheet(&mut self, target_id: usize) {
        let node = &self.doc.nodes[target_id];

        let rel_attr = node.attr(local_name!("rel"));
        let href_attr = node.attr(local_name!("href"));

        let (Some(rels), Some(href)) = (rel_attr, href_attr) else {
            return;
        };
        if !rels.split_ascii_whitespace().any(|rel| rel == "stylesheet") {
            return;
        }

        let url = self.doc.resolve_url(href);
        self.doc.net_provider.fetch(
            self.doc.id(),
            Request::get(url.clone()),
            Box::new(CssHandler {
                node: target_id,
                source_url: url,
                guard: self.doc.guard.clone(),
                provider: self.doc.net_provider.clone(),
            }),
        );
    }

    fn load_image(&mut self, target_id: usize) {
        let node = &self.doc.nodes[target_id];
        if let Some(raw_src) = node.attr(local_name!("src")) {
            if !raw_src.is_empty() {
                let src = self.doc.resolve_url(raw_src);
                self.doc.net_provider.fetch(
                    self.doc.id(),
                    Request::get(src),
                    Box::new(ImageHandler::new(target_id, ImageType::Image)),
                );
            }
        }
    }

    fn process_button_input(&mut self, target_id: usize) {
        let node = &self.doc.nodes[target_id];
        let Some(data) = node.element_data() else {
            return;
        };

        let tagname = data.name.local.as_ref();
        let type_attr = data.attr(local_name!("type"));
        let value = data.attr(local_name!("value"));

        // Add content of "value" attribute as a text node child if:
        //   - Tag name is
        if let ("input", Some("button" | "submit" | "reset"), Some(value)) =
            (tagname, type_attr, value)
        {
            let value = value.to_string();
            let id = self.create_text_node(&value);
            self.append_children(target_id, &[id]);
        }
    }
}

/// Set 'checked' state on an input based on given attributevalue
fn set_input_checked_state(element: &mut ElementNodeData, value: String) {
    let Ok(checked) = value.parse() else {
        return;
    };
    match element.node_specific_data {
        NodeSpecificData::CheckboxInput(ref mut checked_mut) => *checked_mut = checked,
        // If we have just constructed the element, set the node attribute,
        // and NodeSpecificData will be created from that later
        // this simulates the checked attribute being set in html,
        // and the element's checked property being set from that
        NodeSpecificData::None => element.attrs.push(Attribute {
            name: QualName {
                prefix: None,
                ns: ns!(html),
                local: local_name!("checked"),
            },
            value: checked.to_string(),
        }),
        _ => {}
    }
}

#[cfg(feature = "autofocus")]
fn process_cloned_node(doc: &BaseDocument, node_to_autofocus: &mut Option<usize>, node_id: usize) {
    if let Some(node) = doc.get_node(node_id) {
        if node.is_focussable() {
            if let NodeData::Element(ref element) = node.data {
                if let Some(value) = element.attr(local_name!("autofocus")) {
                    if value == "true" {
                        *node_to_autofocus = Some(node_id);
                    }
                }
            }
        }

        for child_node_id in &node.children {
            process_cloned_node(doc, node_to_autofocus, *child_node_id);
        }
    }
}
