use blitz_traits::node_id::NodeId;
use std::collections::HashSet;
use std::mem;
use std::ops::{Deref, DerefMut};

use crate::document::make_device;
use crate::layout::damage::ALL_DAMAGE;
use crate::net::{ImageHandler, ResourceHandler, StylesheetHandler};
use crate::node::{CanvasData, NodeFlags, SpecialElementData};
use crate::util::ImageType;
use crate::{
    Attribute, BaseDocument, Document, ElementData, Node, NodeData, QualName, local_name, qual_name,
};
use blitz_traits::shell::Viewport;
use style::Atom;
use style::invalidation::element::restyle_hints::RestyleHint;
use style::stylesheets::OriginSet;
use thin_vec::ThinVec;

macro_rules! tag_and_attr {
    ($tag:tt, $attr:tt) => {
        (&local_name!($tag), &local_name!($attr))
    };
}

#[derive(Debug, Clone)]
pub enum AppendTextErr {
    /// The node is not a text node
    NotTextNode,
}

/// Operations that happen almost immediately, but are deferred within a
/// function for borrow-checker reasons.
enum SpecialOp {
    LoadImage(NodeId),
    LoadIframe(NodeId),
    LoadStylesheet(NodeId),
    UnloadStylesheet(NodeId),
    LoadCustomPaintSource(NodeId),
    ProcessButtonInput(NodeId),
    UnloadSubDocument(NodeId),
    #[cfg(feature = "custom-widget")]
    UnloadCustomWidget(NodeId),
}

pub struct DocumentMutator<'doc> {
    /// Document is public as an escape hatch, but users of this API should ideally avoid using it
    /// and prefer exposing additional functionality in DocumentMutator.
    pub doc: &'doc mut BaseDocument,

    eager_op_queue: Vec<SpecialOp>,

    // Tracked nodes for deferred processing when mutations have completed
    title_node: Option<NodeId>,
    style_nodes: HashSet<NodeId>,
    form_nodes: HashSet<NodeId>,

    /// Whether an element/attribute that affect animation status has been seen
    recompute_is_animating: bool,

    /// Whether any mutation that affects rendered output has been performed
    mutations_occurred: bool,

    /// The (latest) node which has been mounted in and had autofocus=true, if any
    #[cfg(feature = "autofocus")]
    node_to_autofocus: Option<NodeId>,
}

impl Drop for DocumentMutator<'_> {
    fn drop(&mut self) {
        self.flush(); // Defined at bottom of file
        if self.mutations_occurred {
            self.doc.shell_provider.request_redraw();
        }
    }
}

impl DocumentMutator<'_> {
    pub fn new<'doc>(doc: &'doc mut BaseDocument) -> DocumentMutator<'doc> {
        DocumentMutator {
            doc,
            eager_op_queue: Vec::new(),
            title_node: None,
            style_nodes: HashSet::new(),
            form_nodes: HashSet::new(),
            recompute_is_animating: false,
            mutations_occurred: false,
            #[cfg(feature = "autofocus")]
            node_to_autofocus: None,
        }
    }

    // Query methods

    pub fn node_has_parent(&self, node_id: NodeId) -> bool {
        self.doc.nodes[node_id].parent.is_some()
    }

    pub fn previous_sibling_id(&self, node_id: NodeId) -> Option<NodeId> {
        self.doc.nodes[node_id].backward(1).map(|node| node.id)
    }

    pub fn next_sibling_id(&self, node_id: NodeId) -> Option<NodeId> {
        self.doc.nodes[node_id].forward(1).map(|node| node.id)
    }

    pub fn parent_id(&self, node_id: NodeId) -> Option<NodeId> {
        self.doc.nodes[node_id].parent
    }

    pub fn last_child_id(&self, node_id: NodeId) -> Option<NodeId> {
        self.doc.nodes[node_id].children.last().copied()
    }

    pub fn child_ids(&self, node_id: NodeId) -> ThinVec<NodeId> {
        self.doc.nodes[node_id].children.clone()
    }

    pub fn element_name(&self, node_id: NodeId) -> Option<&QualName> {
        self.doc.nodes[node_id].element_data().map(|el| &el.name)
    }

    pub fn node_at_path(&self, start_node_id: NodeId, path: &[u8]) -> NodeId {
        let mut current = &self.doc.nodes[start_node_id];
        for i in path {
            let new_id = current.children[*i as usize];
            current = &self.doc.nodes[new_id];
        }
        current.id
    }

    // Node creation methods

    pub fn create_comment_node(&mut self, contents: &str) -> NodeId {
        self.doc.create_node(NodeData::Comment {
            contents: contents.to_string(),
        })
    }

    pub fn create_text_node(&mut self, text: &str) -> NodeId {
        self.doc.create_text_node(text)
    }

    pub fn create_element(&mut self, name: QualName, attrs: Vec<Attribute>) -> NodeId {
        let mut data = ElementData::new(name, attrs);
        data.flush_style_attribute(self.doc.guard(), &self.doc.url.url_extra_data());

        let id = self.doc.create_node(NodeData::Element(Box::new(data)));
        let node = self.doc.get_node_mut(id).unwrap();

        // Initialise style data
        *node.stylo_element_data_mut().ensure_init_mut() = style::data::ElementData {
            damage: ALL_DAMAGE,
            ..Default::default()
        };

        id
    }

    pub fn deep_clone_node(&mut self, node_id: NodeId) -> NodeId {
        self.doc.deep_clone_node(node_id)
    }

    // Node mutation methods

    pub fn set_node_text(&mut self, node_id: NodeId, value: &str) {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        let node = &mut self.doc.nodes[node_id];

        let text = match node.data {
            NodeData::Text(ref mut text) => text,
            // TODO: otherwise this is basically element.textContent which is a bit different - need to parse as html
            _ => return,
        };

        let changed = text.content != value;
        if changed {
            self.mutations_occurred |= node_is_in_document;
            text.content.clear();
            text.content.push_str(value);
            node.insert_damage(ALL_DAMAGE);
            // Mark ancestors dirty so the style traversal visits this subtree.
            // Without this, the traversal may skip nodes with pending damage.
            node.mark_ancestors_dirty();
            let parent_id = node.parent;

            // Also insert damage on the parent element, since text content changes
            // affect the parent's layout (text may wrap differently, change size, etc.)
            if let Some(parent_id) = parent_id {
                let parent = &mut self.doc.nodes[parent_id];
                parent.insert_damage(ALL_DAMAGE);
            }

            self.maybe_record_node(parent_id);
        }
    }

    pub fn append_text_to_node(
        &mut self,
        node_id: NodeId,
        text: &str,
    ) -> Result<(), AppendTextErr> {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        let node = &mut self.doc.nodes[node_id];
        node.insert_damage(ALL_DAMAGE);
        node.mark_ancestors_dirty();
        match node.text_data_mut() {
            Some(data) => {
                data.content += text;
                self.mutations_occurred |= node_is_in_document;
                Ok(())
            }
            None => Err(AppendTextErr::NotTextNode),
        }
    }

    pub fn add_attrs_if_missing(&mut self, node_id: NodeId, attrs: Vec<Attribute>) {
        let node = &mut self.doc.nodes[node_id];
        node.insert_damage(ALL_DAMAGE);
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

    pub fn set_attribute(&mut self, node_id: NodeId, name: QualName, value: &str) {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        if node_is_in_document {
            self.doc.snapshot_node(node_id);

            let node = &mut self.doc.nodes[node_id];
            if let Some(mut data) = node.stylo_element_data_opt_mut().and_then(|s| s.get_mut()) {
                data.hint |= RestyleHint::restyle_subtree();
                data.damage.insert(ALL_DAMAGE);
            }

            // TODO: make this fine grained / conditional based on ElementSelectorFlags
            let parent = node.parent;
            if let Some(parent_id) = parent {
                let parent = &mut self.doc.nodes[parent_id];
                if let Some(mut data) = parent
                    .stylo_element_data_opt_mut()
                    .and_then(|s| s.get_mut())
                {
                    data.hint |= RestyleHint::restyle_subtree();
                }
            }

            // Mark ancestors dirty so the style traversal visits this subtree.
            // Without this, the traversal may skip nodes with pending RestyleHint/damage
            // because it uses dirty_descendants flags to determine which subtrees to visit.
            self.doc.nodes[node_id].mark_ancestors_dirty();
        }

        let node = &mut self.doc.nodes[node_id];

        let NodeData::Element(ref mut element) = node.data else {
            return;
        };

        self.mutations_occurred |= node_is_in_document;
        // If element is a CustomWidget, then Ccall attribute_changed on it
        #[cfg(feature = "custom-widget")]
        if let SpecialElementData::CustomWidget(widget_data) = &mut element.special_data {
            let old_value = element.attrs.get(&name).as_ref().map(|attr| &*attr.value);
            widget_data
                .widget
                .attribute_changed(&name.local, old_value, Some(value));
        }

        element.attrs.set(name.clone(), value);

        // Focusability is cached on the element and comes from these
        // attributes, so it has to follow a change to one of them: a widget
        // that hands the focus around its own children - a menu, a grid -
        // sets their tabindex after creating them.
        if name.local == local_name!("tabindex")
            || name.local == local_name!("href")
            || name.local == local_name!("disabled")
        {
            element.flush_is_focussable();
        }

        let tag = &element.name.local;
        let attr = &name.local;

        if *attr == local_name!("id") {
            element.id = Some(Atom::from(value))
        }

        if *attr == local_name!("value") {
            if let Some(input_data) = element.text_input_data_mut() {
                // Update text input value
                input_data.set_text(
                    &mut self.doc.font_ctx.lock().unwrap(),
                    &mut self.doc.layout_ctx,
                    value,
                );
            }
            return;
        }

        if *attr == local_name!("style") {
            element.flush_style_attribute(&self.doc.guard, &self.doc.url.url_extra_data());
            node.mark_style_attr_updated();
            return;
        }

        if *attr == local_name!("disabled") && element.can_be_disabled() {
            node.disable();
            return;
        }

        // If node if not in the document, then don't apply any special behaviours
        // and simply set the attribute value
        if !node.flags.is_in_document() {
            return;
        }

        if (tag, attr) == tag_and_attr!("input", "checked") {
            set_input_checked_state(element, value.to_string());
        } else if (tag, attr) == tag_and_attr!("img", "src") {
            self.load_image(node_id);
        } else if (tag, attr) == tag_and_attr!("canvas", "src") {
            self.load_custom_paint_src(node_id);
        } else if (tag, attr) == tag_and_attr!("link", "href") {
            self.load_linked_stylesheet(node_id);
        } else if (tag, attr) == tag_and_attr!("iframe", "src")
            || (tag, attr) == tag_and_attr!("iframe", "srcdoc")
        {
            self.load_iframe(node_id);
        }
    }

    pub fn clear_attribute(&mut self, node_id: NodeId, name: QualName) {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        if node_is_in_document {
            self.doc.snapshot_node(node_id);

            let node = &mut self.doc.nodes[node_id];

            if let Some(mut data) = node.stylo_element_data_opt_mut().and_then(|s| s.get_mut()) {
                data.hint |= RestyleHint::restyle_subtree();
                data.damage.insert(ALL_DAMAGE);
            }

            // Mark ancestors dirty so the style traversal visits this subtree.
            // Without this, the traversal may skip nodes with pending RestyleHint/damage.
            node.mark_ancestors_dirty();
        }

        let node = &mut self.doc.nodes[node_id];

        let Some(element) = node.element_data_mut() else {
            return;
        };

        let removed_attr = element.attrs.remove(&name);
        let had_attr = removed_attr.is_some();
        if !had_attr {
            return;
        }
        self.mutations_occurred |= node_is_in_document;

        // If element is a CustomWidget, then call attribute_changed on it
        #[cfg(feature = "custom-widget")]
        if let SpecialElementData::CustomWidget(widget_data) = &mut element.special_data {
            let old_value = removed_attr.as_ref().map(|attr| &*attr.value);
            widget_data
                .widget
                .attribute_changed(&name.local, old_value, None);
        }

        if name.local == local_name!("id") {
            element.id = None;
        }

        // As in `set_attribute`: taking one of these away can make the element
        // unfocusable again.
        if name.local == local_name!("tabindex")
            || name.local == local_name!("href")
            || name.local == local_name!("disabled")
        {
            element.flush_is_focussable();
        }

        // Update text input value
        if name.local == local_name!("value") {
            if let Some(input_data) = element.text_input_data_mut() {
                input_data.set_text(
                    &mut self.doc.font_ctx.lock().unwrap(),
                    &mut self.doc.layout_ctx,
                    "",
                );
            }
        }

        let tag = &element.name.local;
        let attr = &name.local;

        if *attr == local_name!("disabled") && element.can_be_disabled() {
            node.enable();
            return;
        }

        if *attr == local_name!("style") {
            element.flush_style_attribute(&self.doc.guard, &self.doc.url.url_extra_data());
            node.mark_style_attr_updated();
        } else if (tag, attr) == tag_and_attr!("canvas", "src") {
            self.recompute_is_animating = true;
        } else if (tag, attr) == tag_and_attr!("link", "href") {
            self.unload_stylesheet(node_id);
        } else if (tag, attr) == tag_and_attr!("iframe", "srcdoc") && node_is_in_document {
            // Fall back to loading from the `src` attribute (if any)
            self.load_iframe(node_id);
        }
    }

    pub fn set_style_property(&mut self, node_id: NodeId, name: &str, value: &str) {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        self.doc.set_style_property(node_id, name, value);
        self.mutations_occurred |= node_is_in_document;
    }

    pub fn remove_style_property(&mut self, node_id: NodeId, name: &str) {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        self.doc.remove_style_property(node_id, name);
        self.mutations_occurred |= node_is_in_document;
    }

    pub fn set_sub_document(&mut self, node_id: NodeId, sub_document: Box<dyn Document>) {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        self.doc.set_sub_document(node_id, sub_document);
        self.mutations_occurred |= node_is_in_document;
    }

    pub fn remove_sub_document(&mut self, node_id: NodeId) {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        self.doc.remove_sub_document(node_id);
        self.mutations_occurred |= node_is_in_document;
    }

    #[cfg(feature = "custom-widget")]
    pub fn set_custom_widget(&mut self, node_id: NodeId, widget: Box<dyn crate::Widget>) {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        self.doc.set_custom_widget(node_id, widget);
        self.mutations_occurred |= node_is_in_document;
    }

    #[cfg(feature = "custom-widget")]
    pub fn remove_custom_widget(&mut self, node_id: NodeId) {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        self.doc.remove_custom_widget(node_id);
        self.mutations_occurred |= node_is_in_document;
    }

    /// Remove the node from it's parent but don't drop it
    pub fn remove_node(&mut self, node_id: NodeId) {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        // Process the subtree *before* severing the parent link so that
        // interaction state referencing removed nodes can retarget to the
        // nearest surviving ancestor.
        self.process_removed_subtree(node_id);

        let node = &mut self.doc.nodes[node_id];

        // Update child_idx values
        if let Some(parent_id) = node.parent.take() {
            self.mutations_occurred |= node_is_in_document;
            let parent = &mut self.doc.nodes[parent_id];
            parent.insert_damage(ALL_DAMAGE);
            // Mark ancestors dirty so the style traversal visits this subtree.
            parent.mark_ancestors_dirty();
            parent.children.retain(|id| *id != node_id);
            self.maybe_record_node(parent_id);
        }
    }

    pub fn remove_and_drop_node(&mut self, node_id: NodeId) -> Option<Node> {
        self.remove_and_drop_node_with(node_id, &mut |_| {})
    }

    /// Like [`Self::remove_and_drop_node`], but calls `on_drop` with the id of
    /// every dropped node (the node itself and all of its descendants).
    pub fn remove_and_drop_node_with(
        &mut self,
        node_id: NodeId,
        on_drop: &mut dyn FnMut(NodeId),
    ) -> Option<Node> {
        let node_is_in_document = self.doc.nodes[node_id].flags.is_in_document();
        self.process_removed_subtree(node_id);

        let node = self.doc.drop_node_ignoring_parent_with(node_id, on_drop);
        self.mutations_occurred |= node_is_in_document;

        // Update child_idx values
        if let Some(parent_id) = node.as_ref().and_then(|node| node.parent) {
            let parent = &mut self.doc.nodes[parent_id];
            parent.insert_damage(ALL_DAMAGE);
            let parent_is_in_doc = parent.flags.is_in_document();

            // TODO: make this fine grained / conditional based on ElementSelectorFlags
            if parent_is_in_doc {
                if let Some(mut data) = parent
                    .stylo_element_data_opt_mut()
                    .and_then(|s| s.get_mut())
                {
                    data.hint |= RestyleHint::restyle_subtree();
                }
                // Mark ancestors dirty so the style traversal visits this subtree.
                parent.mark_ancestors_dirty();
            }

            parent.children.retain(|id| *id != node_id);
            self.maybe_record_node(parent_id);
        }

        node
    }

    pub fn remove_and_drop_all_children(&mut self, node_id: NodeId) {
        let parent = &mut self.doc.nodes[node_id];
        let parent_is_in_doc = parent.flags.is_in_document();

        // TODO: make this fine grained / conditional based on ElementSelectorFlags
        if parent_is_in_doc {
            if let Some(mut data) = parent
                .stylo_element_data_opt_mut()
                .and_then(|s| s.get_mut())
            {
                data.hint |= RestyleHint::restyle_subtree();
            }
            // Mark ancestors dirty so the style traversal visits this subtree.
            parent.mark_ancestors_dirty();
        }

        let children = mem::take(&mut parent.children);
        self.mutations_occurred |= parent_is_in_doc && !children.is_empty();
        for child_id in children {
            self.process_removed_subtree(child_id);
            let _ = self.doc.drop_node_ignoring_parent(child_id);
        }
        self.maybe_record_node(node_id);
    }

    // Tree mutation methods
    pub fn remove_node_if_unparented(&mut self, node_id: NodeId) {
        self.remove_node_if_unparented_with(node_id, &mut |_| {});
    }

    /// Like [`Self::remove_node_if_unparented`], but calls `on_drop` with the id of
    /// every dropped node (the node itself and all of its descendants).
    pub fn remove_node_if_unparented_with(
        &mut self,
        node_id: NodeId,
        on_drop: &mut dyn FnMut(NodeId),
    ) {
        if let Some(node) = self.doc.get_node(node_id) {
            if node.parent.is_none() {
                self.remove_and_drop_node_with(node_id, on_drop);
            }
        }
    }

    /// Remove all of the children from old_parent_id and append them to new_parent_id
    pub fn append_children(&mut self, parent_id: NodeId, child_ids: &[NodeId]) {
        self.add_children_to_parent(parent_id, child_ids, &|parent, child_ids| {
            parent.children.extend_from_slice(child_ids);
        });
    }

    pub fn insert_nodes_before(&mut self, anchor_node_id: NodeId, new_node_ids: &[NodeId]) {
        let parent_id = self.doc.nodes[anchor_node_id].parent.unwrap();
        self.add_children_to_parent(parent_id, new_node_ids, &|parent, child_ids| {
            let node_child_idx = parent.index_of_child(anchor_node_id).unwrap();
            parent
                .children
                .splice(node_child_idx..node_child_idx, child_ids.iter().copied());
        });
    }

    fn add_children_to_parent(
        &mut self,
        parent_id: NodeId,
        child_ids: &[NodeId],
        insert_children_fn: &dyn Fn(&mut Node, &[NodeId]),
    ) {
        let new_parent_is_in_document = self.doc.nodes[parent_id].flags.is_in_document();
        self.mutations_occurred |= new_parent_is_in_document && !child_ids.is_empty();
        // Detach the children from their old parents *before* inserting them into
        // the new parent (matching DOM `insertBefore` semantics). If a child is
        // being moved within the same parent then detaching it after insertion
        // would remove both the old and the newly-inserted entries from the
        // parent's child list, and anchor indices would be computed against a
        // child list that still contains the moved nodes.
        for child_id in child_ids.iter().copied() {
            let child = &mut self.doc.nodes[child_id];
            let child_was_in_doc = child.flags.is_in_document();
            self.mutations_occurred |= child_was_in_doc;
            let Some(old_parent_id) = child.parent.take() else {
                continue;
            };

            let old_parent = &mut self.doc.nodes[old_parent_id];
            old_parent.insert_damage(ALL_DAMAGE);

            // TODO: make this fine grained / conditional based on ElementSelectorFlags
            if child_was_in_doc {
                if let Some(mut data) = old_parent
                    .stylo_element_data_opt_mut()
                    .and_then(|s| s.get_mut())
                {
                    data.hint |= RestyleHint::restyle_subtree();
                }
                // Mark ancestors dirty so the style traversal visits this subtree.
                old_parent.mark_ancestors_dirty();
            }

            old_parent.children.retain(|id| *id != child_id);
            self.maybe_record_node(old_parent_id);
        }

        let new_parent = &mut self.doc.nodes[parent_id];
        new_parent.insert_damage(ALL_DAMAGE);

        // TODO: make this fine grained / conditional based on ElementSelectorFlags
        if new_parent_is_in_document {
            if let Some(mut data) = new_parent
                .stylo_element_data_opt_mut()
                .and_then(|s| s.get_mut())
            {
                data.hint |= RestyleHint::restyle_subtree();
            }
            // Mark ancestors dirty so the style traversal visits this subtree.
            new_parent.mark_ancestors_dirty();
        }

        insert_children_fn(new_parent, child_ids);

        for child_id in child_ids.iter().copied() {
            let child = &mut self.doc.nodes[child_id];
            let child_was_in_doc = child.flags.is_in_document();
            child.parent = Some(parent_id);

            if new_parent_is_in_document && !child_was_in_doc {
                self.process_added_subtree(child_id);
            } else if !new_parent_is_in_document && child_was_in_doc {
                self.process_removed_subtree(child_id);
            }
        }

        self.maybe_record_node(parent_id);
    }

    // Tree mutation methods (that defer to other methods)
    pub fn insert_nodes_after(&mut self, anchor_node_id: NodeId, new_node_ids: &[NodeId]) {
        match self.next_sibling_id(anchor_node_id) {
            Some(id) => self.insert_nodes_before(id, new_node_ids),
            None => {
                let parent_id = self.parent_id(anchor_node_id).unwrap();
                self.append_children(parent_id, new_node_ids)
            }
        }
    }

    pub fn reparent_children(&mut self, old_parent_id: NodeId, new_parent_id: NodeId) {
        let child_ids = std::mem::take(&mut self.doc.nodes[old_parent_id].children);
        self.maybe_record_node(old_parent_id);
        self.append_children(new_parent_id, &child_ids);
    }

    pub fn replace_node_with(&mut self, anchor_node_id: NodeId, new_node_ids: &[NodeId]) {
        self.insert_nodes_before(anchor_node_id, new_node_ids);
        self.remove_node(anchor_node_id);
    }
}

impl<'doc> DocumentMutator<'doc> {
    pub fn flush(&mut self) {
        if self.recompute_is_animating {
            self.doc.has_canvas = self.doc.compute_has_canvas();
        }

        if let Some(id) = self.title_node {
            let title = self.doc.nodes[id].text_content();
            self.doc.shell_provider.set_window_title(title);
        }

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

    pub fn set_inner_html(&mut self, node_id: NodeId, html: &str) {
        self.remove_and_drop_all_children(node_id);
        self.doc
            .html_parser_provider
            .clone()
            .parse_inner_html(self, node_id, html);
    }

    fn flush_eager_ops(&mut self) {
        let mut ops = mem::take(&mut self.eager_op_queue);
        for op in ops.drain(0..) {
            match op {
                SpecialOp::LoadImage(node_id) => self.load_image(node_id),
                SpecialOp::LoadIframe(node_id) => self.load_iframe(node_id),
                SpecialOp::LoadStylesheet(node_id) => self.load_linked_stylesheet(node_id),
                SpecialOp::UnloadStylesheet(node_id) => self.unload_stylesheet(node_id),
                SpecialOp::LoadCustomPaintSource(node_id) => self.load_custom_paint_src(node_id),
                SpecialOp::ProcessButtonInput(node_id) => self.process_button_input(node_id),
                SpecialOp::UnloadSubDocument(node_id) => self.remove_sub_document(node_id),
                #[cfg(feature = "custom-widget")]
                SpecialOp::UnloadCustomWidget(node_id) => self.remove_custom_widget(node_id),
            }
        }

        // Queue is empty, but put Vec back anyway so allocation can be reused.
        self.eager_op_queue = ops;
    }

    fn process_added_subtree(&mut self, node_id: NodeId) {
        self.doc.iter_subtree_mut(node_id, |node_id, doc| {
            let node = &mut doc.nodes[node_id];
            node.flags.set(NodeFlags::IS_IN_DOCUMENT, true);
            node.insert_damage(ALL_DAMAGE);

            // If the node has an "id" attribute, store it in the ID map.
            if let Some(id_attr) = node.attr(local_name!("id")) {
                doc.nodes_to_id.insert(id_attr.to_string(), node_id);
            }

            let NodeData::Element(ref mut element) = node.data else {
                return;
            };

            // Custom post-processing by element tag name
            let tag = element.name.local.as_ref();
            match tag {
                "title" => self.title_node = Some(node_id),
                "link" => self.eager_op_queue.push(SpecialOp::LoadStylesheet(node_id)),
                "img" => self.eager_op_queue.push(SpecialOp::LoadImage(node_id)),
                "iframe" => self.eager_op_queue.push(SpecialOp::LoadIframe(node_id)),
                "canvas" => self
                    .eager_op_queue
                    .push(SpecialOp::LoadCustomPaintSource(node_id)),
                "style" => {
                    self.style_nodes.insert(node_id);
                }
                "button" | "fieldset" | "input" | "select" | "textarea" | "object" | "output" => {
                    self.eager_op_queue
                        .push(SpecialOp::ProcessButtonInput(node_id));
                    self.form_nodes.insert(node_id);
                }
                _ => {}
            }

            #[cfg(feature = "autofocus")]
            if node.is_focussable() {
                if let NodeData::Element(ref element) = node.data {
                    if let Some(value) = element.attr(local_name!("autofocus")) {
                        if value == "true" {
                            self.node_to_autofocus = Some(node_id);
                        }
                    }
                }
            }
        });

        self.flush_eager_ops();
    }

    fn process_removed_subtree(&mut self, node_id: NodeId) {
        self.doc.iter_subtree_mut(node_id, |node_id, doc| {
            doc.nodes[node_id]
                .flags
                .set(NodeFlags::IS_IN_DOCUMENT, false);

            // Clear any interaction state that references this node, running
            // the usual teardown steps (unhover/unactive the surviving
            // ancestor chain, IME disable on blur of a focused input).
            doc.clear_interaction_state_for_removed_node(node_id);

            let node = &mut doc.nodes[node_id];

            // Remove any snapshot for this node to prevent stale snapshot references
            // during style invalidation.
            if node.has_snapshot() {
                let opaque_id = style::dom::TNode::opaque(&&*node);
                doc.snapshots.remove(&opaque_id);
                node.set_has_snapshot(false);
            }

            // If the node has an "id" attribute remove it from the ID map.
            if let Some(id_attr) = node.attr(local_name!("id")) {
                doc.nodes_to_id.remove(id_attr);
            }

            let NodeData::Element(ref mut element) = node.data else {
                return;
            };

            match &element.special_data {
                SpecialElementData::SubDocument(_) => {
                    self.eager_op_queue
                        .push(SpecialOp::UnloadSubDocument(node_id));
                }
                #[cfg(feature = "custom-widget")]
                SpecialElementData::CustomWidget(_) => {
                    self.eager_op_queue
                        .push(SpecialOp::UnloadCustomWidget(node_id));
                }
                SpecialElementData::Stylesheet(_) => self
                    .eager_op_queue
                    .push(SpecialOp::UnloadStylesheet(node_id)),
                SpecialElementData::Image(_) => {}
                SpecialElementData::Canvas(_) => {
                    self.recompute_is_animating = true;
                }
                SpecialElementData::TableRoot(_) => {}
                SpecialElementData::TextInput(_) => {}
                SpecialElementData::CheckboxInput(_) => {}
                #[cfg(feature = "file-input")]
                SpecialElementData::FileInput(_) => {}
                SpecialElementData::None => {}
            }
        });

        self.flush_eager_ops();
    }

    fn maybe_record_node(&mut self, node_id: impl Into<Option<NodeId>>) {
        let Some(node_id) = node_id.into() else {
            return;
        };

        let Some(tag_name) = self.doc.nodes[node_id]
            .data
            .downcast_element()
            .map(|elem| &elem.name.local)
        else {
            return;
        };

        match tag_name.as_ref() {
            "title" => self.title_node = Some(node_id),
            "style" => {
                self.style_nodes.insert(node_id);
            }
            _ => {}
        }
    }

    fn load_linked_stylesheet(&mut self, target_id: NodeId) {
        let node = &self.doc.nodes[target_id];

        let mut is_in_head = false;
        let mut parent_id = node.parent;
        while let Some(id) = parent_id
            && !is_in_head
        {
            let parent = &self.doc.nodes[id];
            is_in_head |= parent.data.is_element_with_tag_name(&local_name!("head"));
            parent_id = parent.parent;
        }

        let rel_attr = node.attr(local_name!("rel"));
        let href_attr = node.attr(local_name!("href"));

        let (Some(rels), Some(href)) = (rel_attr, href_attr) else {
            return;
        };
        if !rels.split_ascii_whitespace().any(|rel| rel == "stylesheet") {
            return;
        }

        let url = self.doc.resolve_url(href);
        let handler = ResourceHandler::new(
            self.doc.tx.clone(),
            self.doc.id(),
            Some(node.id),
            self.doc.shell_provider.clone(),
            StylesheetHandler {
                source_url: url.clone(),
                guard: self.doc.guard.clone(),
                net_provider: self.doc.net_provider.clone(),
                abort_signal: self.doc.abort_signal.clone(),
            },
        );

        if is_in_head && !self.doc.net_provider.is_noop() {
            self.doc
                .pending_critical_resources
                .insert(handler.request_id());
        }

        self.doc.net_provider.fetch(
            self.doc.id(),
            self.doc.build_request(url),
            Box::new(handler),
        );
    }

    fn unload_stylesheet(&mut self, node_id: NodeId) {
        let node = &mut self.doc.nodes[node_id];
        let Some(element) = node.element_data_mut() else {
            unreachable!();
        };
        let SpecialElementData::Stylesheet(stylesheet) = element.special_data.take() else {
            unreachable!();
        };

        let guard = self.doc.guard.read();
        self.doc.stylist.remove_stylesheet(stylesheet, &guard);
        self.doc
            .stylist
            .force_stylesheet_origins_dirty(OriginSet::all());

        self.doc.nodes_to_stylesheet.remove(&node_id);
    }

    fn load_image(&mut self, target_id: NodeId) {
        let node = &self.doc.nodes[target_id];
        if let Some(raw_src) = node.attr(local_name!("src")) {
            if !raw_src.is_empty() {
                let src = self.doc.resolve_url(raw_src);
                let src_string = src.as_str();

                // Check cache first
                if let Some(cached_image) = self.doc.image_cache.get(src_string) {
                    #[cfg(feature = "tracing")]
                    tracing::info!("Loading image {src_string} from cache");
                    let node = &mut self.doc.nodes[target_id];
                    node.element_data_mut().unwrap().special_data =
                        SpecialElementData::Image(Box::new(cached_image.clone()));
                    node.cache_mut().clear();
                    node.insert_damage(ALL_DAMAGE);
                    return;
                }

                // Check if there's already a pending request for this URL
                if let Some(waiting_list) = self.doc.pending_images.get_mut(src_string) {
                    #[cfg(feature = "tracing")]
                    tracing::info!("Image {src_string} already pending, queueing node {target_id}");
                    waiting_list.push((target_id, ImageType::Image));
                    return;
                }

                // Start fetch and track as pending
                #[cfg(feature = "tracing")]
                tracing::info!("Fetching image {src_string}");
                self.doc
                    .pending_images
                    .insert(src_string.to_string(), vec![(target_id, ImageType::Image)]);

                self.doc.net_provider.fetch(
                    self.doc.id(),
                    self.doc.build_request(src),
                    ResourceHandler::boxed(
                        self.doc.tx.clone(),
                        self.doc.id(),
                        None, // Don't pass node_id, we'll handle it via pending_images
                        self.doc.shell_provider.clone(),
                        ImageHandler::new(ImageType::Image),
                    ),
                );
            }
        }
    }

    fn load_iframe(&mut self, target_id: NodeId) {
        if self.doc.subdocument_depth >= crate::iframe::MAX_SUBDOCUMENT_DEPTH {
            #[cfg(feature = "tracing")]
            tracing::warn!(
                "Not loading iframe: max sub-document nesting depth ({}) reached",
                crate::iframe::MAX_SUBDOCUMENT_DEPTH
            );
            return;
        }

        let node = &self.doc.nodes[target_id];
        let Some(element) = node.element_data() else {
            return;
        };

        // `srcdoc` takes precedence over `src`
        if let Some(srcdoc) = element.attr(local_name!("srcdoc")) {
            let srcdoc = srcdoc.to_string();
            self.doc.load_iframe_srcdoc(target_id, &srcdoc);
            return;
        }

        let Some(raw_src) = element.attr(local_name!("src")) else {
            return;
        };
        if raw_src.is_empty() {
            return;
        }
        let Some(url) = self.doc.url.resolve_relative(raw_src) else {
            #[cfg(feature = "tracing")]
            tracing::warn!("Not loading iframe: could not resolve url {raw_src}");
            return;
        };
        self.doc.start_iframe_load(target_id, url);
    }

    fn load_custom_paint_src(&mut self, target_id: NodeId) {
        let node = &mut self.doc.nodes[target_id];
        if let Some(raw_src) = node.attr(local_name!("src")) {
            if let Ok(custom_paint_source_id) = raw_src.parse::<u64>() {
                self.recompute_is_animating = true;
                let canvas_data = SpecialElementData::Canvas(CanvasData {
                    custom_paint_source_id,
                });
                node.element_data_mut().unwrap().special_data = canvas_data;
            }
        }
    }

    fn process_button_input(&mut self, target_id: NodeId) {
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
            return;
        }
        #[cfg(feature = "file-input")]
        if let ("input", Some("file")) = (tagname, type_attr) {
            let button_id = self.create_element(
                qual_name!("button", html),
                vec![
                    Attribute {
                        name: qual_name!("type", html),
                        value: "button".to_string(),
                    },
                    Attribute {
                        name: qual_name!("tabindex", html),
                        value: "-1".to_string(),
                    },
                ],
            );
            let label_id = self.create_element(qual_name!("label", html), vec![]);
            let text_id = self.create_text_node("No File Selected");
            let button_text_id = self.create_text_node("Browse");
            self.append_children(target_id, &[button_id, label_id]);
            self.append_children(label_id, &[text_id]);
            self.append_children(button_id, &[button_text_id]);
        }
    }
}

/// Set 'checked' state on an input based on given attributevalue
fn set_input_checked_state(element: &mut ElementData, value: String) {
    let Ok(checked) = value.parse() else {
        return;
    };
    match element.special_data {
        SpecialElementData::CheckboxInput(ref mut checked_mut) => *checked_mut = checked,
        // If we have just constructed the element, set the node attribute,
        // and NodeSpecificData will be created from that later
        // this simulates the checked attribute being set in html,
        // and the element's checked property being set from that
        SpecialElementData::None => element.attrs.push(Attribute {
            name: qual_name!("checked", html),
            value: checked.to_string(),
        }),
        _ => {}
    }
}

/// Type that allows mutable access to the viewport
/// And syncs it back to stylist on drop.
pub struct ViewportMut<'doc> {
    doc: &'doc mut BaseDocument,
    initial_viewport: Viewport,
}
impl ViewportMut<'_> {
    pub fn new(doc: &mut BaseDocument) -> ViewportMut<'_> {
        let initial_viewport = doc.viewport.clone();
        ViewportMut {
            doc,
            initial_viewport,
        }
    }
}
impl Deref for ViewportMut<'_> {
    type Target = Viewport;

    fn deref(&self) -> &Self::Target {
        &self.doc.viewport
    }
}
impl DerefMut for ViewportMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.doc.viewport
    }
}
impl Drop for ViewportMut<'_> {
    fn drop(&mut self) {
        if self.doc.viewport == self.initial_viewport {
            return;
        }

        self.doc.set_stylist_device(make_device(
            &self.doc.viewport,
            self.doc.media_type.clone(),
            self.doc.font_ctx.clone(),
        ));
        self.doc.scroll_viewport_by(0.0, 0.0); // Clamp scroll offset

        let scale_has_changed =
            self.doc.viewport().scale_f64() != self.initial_viewport.scale_f64();
        if scale_has_changed {
            self.doc.invalidate_inline_contexts();
            self.doc.shell_provider.request_redraw();
        }
    }
}

#[cfg(test)]
mod test {
    use style::media_queries::MediaType;
    use style_dom::ElementState;

    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use blitz_traits::shell::{ColorScheme, ShellProvider, Viewport};

    use crate::{Attribute, BaseDocument, DocumentConfig, ElementData, NodeData, qual_name};

    #[test]
    fn media_type_defaults_to_screen() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        assert_eq!(*document.media_type(), MediaType::screen());
        assert_eq!(document.stylist_device().media_type(), MediaType::screen());
    }

    #[test]
    fn media_type_honors_config() {
        let mut document = BaseDocument::new(DocumentConfig {
            media_type: Some(MediaType::print()),
            ..Default::default()
        });
        assert_eq!(*document.media_type(), MediaType::print());
        assert_eq!(document.stylist_device().media_type(), MediaType::print());
    }

    #[test]
    fn set_media_type_updates_stylist_device() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        assert_eq!(document.stylist_device().media_type(), MediaType::screen());

        document.set_media_type(MediaType::print());
        assert_eq!(*document.media_type(), MediaType::print());
        assert_eq!(document.stylist_device().media_type(), MediaType::print());
    }

    #[test]
    fn mutator_remove_disabled() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        let id = document.create_node(NodeData::Element(Box::new(ElementData::new(
            qual_name!("button"),
            vec![Attribute {
                name: qual_name!("disabled"),
                value: "".into(),
            }],
        ))));

        let node = document.get_node(id).unwrap();
        assert!(
            node.element_state().contains(ElementState::DISABLED),
            "form node is disabled"
        );
        assert!(
            !node.element_state().contains(ElementState::ENABLED),
            "form node is not enabled yet"
        );

        let mut mutator = document.mutate();
        mutator.clear_attribute(id, qual_name!("disabled"));
        drop(mutator);

        let node = document.get_node(id).unwrap();
        assert!(
            !node.element_state().contains(ElementState::DISABLED),
            "form node is no longer disabled"
        );
        assert!(
            node.element_state().contains(ElementState::ENABLED),
            "form node is enabled"
        );
    }

    #[test]
    fn mutator_set_disabled() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        let id = document.create_node(NodeData::Element(Box::new(ElementData::new(
            qual_name!("button"),
            vec![],
        ))));

        let node = document.get_node(id).unwrap();
        assert!(
            !node.element_state().contains(ElementState::DISABLED),
            "form node is not disabled"
        );
        assert!(
            node.element_state().contains(ElementState::ENABLED),
            "form node is enabled"
        );

        let mut mutator = document.mutate();
        mutator.set_attribute(id, qual_name!("disabled"), "");
        drop(mutator);

        let node = document.get_node(id).unwrap();

        assert!(
            node.element_state().contains(ElementState::DISABLED),
            "form node is disabled"
        );
        assert!(
            !node.element_state().contains(ElementState::ENABLED),
            "form node is no longer enabled enabled"
        );
    }

    #[test]
    fn mutator_set_disabled_invalid_node() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        let id = document.create_node(NodeData::Element(Box::new(ElementData::new(
            qual_name!("a"),
            vec![],
        ))));

        let node = document.get_node(id).unwrap();
        assert!(
            !node.element_state().contains(ElementState::DISABLED),
            "form node is not disabled"
        );
        assert!(
            !node.element_state().contains(ElementState::ENABLED),
            "form node is enabled"
        );

        let mut mutator = document.mutate();
        mutator.set_attribute(id, qual_name!("disabled"), "");
        drop(mutator);

        let node = document.get_node(id).unwrap();
        assert!(
            !node.element_state().contains(ElementState::DISABLED),
            "form node is not disabled"
        );
        assert!(
            !node.element_state().contains(ElementState::ENABLED),
            "form node is enabled"
        );
    }

    #[derive(Default)]
    struct RedrawShell {
        redraw_requests: AtomicUsize,
    }

    impl ShellProvider for RedrawShell {
        fn request_redraw(&self) {
            self.redraw_requests.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn mutator_requests_redraw_only_after_mutation() {
        let shell = Arc::new(RedrawShell::default());
        let mut document = BaseDocument::new(DocumentConfig {
            shell_provider: Some(shell.clone()),
            ..Default::default()
        });
        let root_id = document.root_node().id;

        {
            let mut mutator = document.mutate();
            let parent_id = mutator.create_element(qual_name!("div"), vec![]);
            let child_id = mutator.create_element(qual_name!("span"), vec![]);
            mutator.append_children(parent_id, &[child_id]);
            mutator.remove_and_drop_all_children(parent_id);
            mutator.set_attribute(parent_id, qual_name!("id"), "detached");
        }
        assert_eq!(shell.redraw_requests.load(Ordering::Relaxed), 0);

        {
            let mutator = document.mutate();
            assert_eq!(mutator.child_ids(root_id).len(), 0);
        }

        {
            let mut mutator = document.mutate();
            let node_id = mutator.create_element(qual_name!("div"), vec![]);
            mutator.append_children(root_id, &[node_id]);
            mutator.set_attribute(node_id, qual_name!("id"), "in-document");
        }
        assert_eq!(shell.redraw_requests.load(Ordering::Relaxed), 1);

        {
            let mut mutator = document.mutate();
            let parent_id = mutator.create_element(qual_name!("div"), vec![]);
            let child_id = mutator.create_element(qual_name!("span"), vec![]);
            mutator.append_children(root_id, &[parent_id]);
            mutator.append_children(parent_id, &[child_id]);
            mutator.remove_and_drop_all_children(parent_id);
        }
        assert_eq!(shell.redraw_requests.load(Ordering::Relaxed), 2);

        {
            let mut mutator = document.mutate();
            let parent_id = mutator.create_element(qual_name!("div"), vec![]);
            let child_id = mutator.create_element(qual_name!("span"), vec![]);
            let detached_target_id = mutator.create_element(qual_name!("div"), vec![]);
            mutator.append_children(root_id, &[parent_id]);
            mutator.append_children(parent_id, &[child_id]);
            assert_eq!(shell.redraw_requests.load(Ordering::Relaxed), 2);
            mutator.append_children(detached_target_id, &[child_id]);
        }
        assert_eq!(shell.redraw_requests.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn moving_subtree_out_of_document_clears_in_document_flag() {
        let shell = Arc::new(RedrawShell::default());
        let mut document = BaseDocument::new(DocumentConfig {
            shell_provider: Some(shell.clone()),
            ..Default::default()
        });
        let root_id = document.root_node().id;
        let (child_id, grandchild_id, detached_parent_id) = {
            let mut mutator = document.mutate();
            let in_document_parent_id = mutator.create_element(qual_name!("div"), vec![]);
            let child_id = mutator.create_element(qual_name!("div"), vec![]);
            let grandchild_id = mutator.create_element(qual_name!("span"), vec![]);
            let detached_parent_id = mutator.create_element(qual_name!("section"), vec![]);
            mutator.append_children(root_id, &[in_document_parent_id]);
            mutator.append_children(in_document_parent_id, &[child_id]);
            mutator.append_children(child_id, &[grandchild_id]);
            (child_id, grandchild_id, detached_parent_id)
        };
        assert_eq!(shell.redraw_requests.load(Ordering::Relaxed), 1);
        assert!(document.get_node(child_id).unwrap().flags.is_in_document());
        assert!(
            document
                .get_node(grandchild_id)
                .unwrap()
                .flags
                .is_in_document()
        );

        {
            let mut mutator = document.mutate();
            mutator.append_children(detached_parent_id, &[child_id]);
        }
        assert_eq!(shell.redraw_requests.load(Ordering::Relaxed), 2);
        assert!(!document.get_node(child_id).unwrap().flags.is_in_document());
        assert!(
            !document
                .get_node(grandchild_id)
                .unwrap()
                .flags
                .is_in_document()
        );

        {
            let mut mutator = document.mutate();
            mutator.set_attribute(child_id, qual_name!("id"), "detached");
        }
        assert_eq!(shell.redraw_requests.load(Ordering::Relaxed), 2);

        {
            let mut mutator = document.mutate();
            mutator.append_children(root_id, &[child_id]);
        }
        assert_eq!(shell.redraw_requests.load(Ordering::Relaxed), 3);
        assert!(document.get_node(child_id).unwrap().flags.is_in_document());
        assert!(
            document
                .get_node(grandchild_id)
                .unwrap()
                .flags
                .is_in_document()
        );
    }

    #[test]
    fn style_property_updates_nested_layout() {
        let mut document = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        let root_id = document.root_node().id;

        let mover_id = {
            let mut mutator = document.mutate();
            let parent_id = mutator.create_element(qual_name!("div"), vec![]);
            let mover_id = mutator.create_element(qual_name!("div"), vec![]);
            mutator.set_style_property(parent_id, "position", "relative");
            mutator.set_style_property(parent_id, "width", "800px");
            mutator.set_style_property(parent_id, "height", "600px");
            mutator.set_style_property(mover_id, "position", "absolute");
            mutator.set_style_property(mover_id, "left", "0px");
            mutator.set_style_property(mover_id, "top", "0px");
            mutator.append_children(parent_id, &[mover_id]);
            mutator.append_children(root_id, &[parent_id]);
            mover_id
        };

        document.resolve(0.0);
        assert_eq!(
            document
                .get_node(mover_id)
                .unwrap()
                .final_layout()
                .location
                .x,
            0.0
        );

        {
            let mut mutator = document.mutate();
            mutator.set_style_property(mover_id, "left", "120px");
        }

        document.resolve(0.0);
        assert_eq!(
            document
                .get_node(mover_id)
                .unwrap()
                .final_layout()
                .location
                .x,
            120.0
        );
    }
}
