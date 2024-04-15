//! Integration between Dioxus and Blitz

use super::DocumentLike;
use blitz_dom::{
    local_name, node::Attribute, ns, Atom, Document, ElementNodeData, Namespace, Node, NodeData,
    QualName, TextNodeData,
};

use dioxus::dioxus_core::{
    self, AttributeValue, ElementId, TemplateAttribute, TemplateNode, WriteMutations,
};
use rustc_hash::{FxHashMap, FxHashSet};

type NodeId = usize;

fn qual_name(local_name: &str, namespace: Option<&str>) -> QualName {
    QualName {
        prefix: None,
        ns: namespace.map(Atom::from).unwrap_or(ns!(html)),
        local: Atom::from(local_name),
    }
}

pub struct DioxusDocument {
    inner: Document,
}

// Implement DocumentLike and required traits for DioxusDocument

impl AsRef<Document> for DioxusDocument {
    fn as_ref(&self) -> &Document {
        &self.inner
    }
}
impl AsMut<Document> for DioxusDocument {
    fn as_mut(&mut self) -> &mut Document {
        &mut self.inner
    }
}
impl Into<Document> for DioxusDocument {
    fn into(self) -> Document {
        self.inner
    }
}
impl DocumentLike for DioxusDocument {}

/// The state of the Dioxus integration with the RealDom
pub struct DioxusState {
    /// Store of templates keyed by unique name
    templates: FxHashMap<String, Vec<NodeId>>,
    /// Stack machine state for applying dioxus mutations
    stack: Vec<NodeId>,
    /// Mapping from vdom ElementId -> rdom NodeId
    node_id_mapping: Vec<Option<NodeId>>,
}

/// A writer for mutations that can be used with the RealDom.
pub struct DioxusNativeCoreMutationWriter<'a> {
    /// The realdom associated with this writer
    pub doc: &'a mut Document,

    /// The state associated with this writer
    pub state: &'a mut DioxusState,
}

impl DioxusState {
    /// Initialize the DioxusState in the RealDom
    pub fn create(doc: &mut Document) -> Self {
        let mut root = doc.root_node();
        let root_id = root.id;
        Self {
            templates: FxHashMap::default(),
            stack: vec![root_id],
            node_id_mapping: vec![Some(root_id)],
        }
    }

    /// Convert an ElementId to a NodeId
    pub fn element_to_node_id(&self, element_id: ElementId) -> NodeId {
        self.try_element_to_node_id(element_id).unwrap()
    }

    /// Attempt to convert an ElementId to a NodeId. This will return None if the ElementId is not in the RealDom.
    pub fn try_element_to_node_id(&self, element_id: ElementId) -> Option<NodeId> {
        self.node_id_mapping.get(element_id.0).copied().flatten()
    }

    /// Create a mutation writer for the RealDom
    pub fn create_mutation_writer<'a>(
        &'a mut self,
        doc: &'a mut Document,
    ) -> DioxusNativeCoreMutationWriter<'a> {
        DioxusNativeCoreMutationWriter { doc, state: self }
    }
}

impl DioxusNativeCoreMutationWriter<'_> {
    /// Update an ElementId -> NodeId mapping
    fn set_id_mapping(&mut self, node_id: NodeId, element_id: ElementId) {
        let element_id: usize = element_id.0;

        // Ensure node_id_mapping is large enough to contain element_id
        if self.state.node_id_mapping.len() <= element_id {
            self.state.node_id_mapping.resize(element_id + 1, None);
        }
        // If element_id is already mapping to a node, remove that node from the document
        else if let Some(mut mapped_node_id) = self.state.node_id_mapping[element_id] {
            self.doc.remove_node(mapped_node_id);
        }

        // Set the new mapping
        self.state.node_id_mapping[element_id] = Some(node_id);
    }

    /// Find a child in the document by child index path
    fn load_child(&self, path: &[u8]) -> NodeId {
        let mut current = self
            .doc
            .get_node(*self.state.stack.last().unwrap())
            .unwrap();
        for i in path {
            let new_id = current.children[*i as usize];
            current = self.doc.get_node(new_id).unwrap();
        }
        current.id
    }
}

impl WriteMutations for DioxusNativeCoreMutationWriter<'_> {
    fn register_template(&mut self, template: dioxus_core::prelude::Template) {
        let template_root_ids: Vec<NodeId> = template
            .roots
            .into_iter()
            .map(|root| create_template_node(self.doc, root))
            .collect();
        self.state
            .templates
            .insert(template.name.to_string(), template_root_ids);
    }

    fn append_children(&mut self, id: ElementId, m: usize) {
        let children = self.state.stack.split_off(self.state.stack.len() - m);
        let parent = self.state.element_to_node_id(id);
        for child in children {
            self.doc.get_node_mut(parent).unwrap().children.push(child);
        }
    }

    fn assign_node_id(&mut self, path: &'static [u8], id: ElementId) {
        let node_id = self.load_child(path);
        self.set_id_mapping(node_id, id);
    }

    fn create_placeholder(&mut self, id: ElementId) {
        let node_id = self.doc.create_node(NodeData::Comment);
        self.set_id_mapping(node_id, id);
        self.state.stack.push(node_id);
    }

    fn create_text_node(&mut self, value: &str, id: ElementId) {
        let node_id = self.doc.create_text_node(value);
        self.set_id_mapping(node_id, id);
        self.state.stack.push(node_id);
    }

    fn hydrate_text_node(&mut self, path: &'static [u8], value: &str, id: ElementId) {
        // let node_id = self.state.load_child(self.rdom, path);
        // let node = self.rdom.get_mut(node_id).unwrap();
        // self.set_id_mapping(node, id);
        // let mut node = self.rdom.get_mut(node_id).unwrap();
        // let node_type_mut = node.node_type_mut();
        // if let NodeTypeMut::Text(mut text) = node_type_mut {
        //     *text.text_mut() = value.to_string();
        // } else {
        //     drop(node_type_mut);
        //     node.set_type(NodeType::Text(TextNode {
        //         text: value.to_string(),
        //         listeners: FxHashSet::default(),
        //     }));
        // }
    }

    fn load_template(&mut self, name: &'static str, index: usize, id: ElementId) {
        let template_node_id = self.state.templates[name][index];
        let clone_id = self.doc.deep_clone_node(template_node_id);
        self.set_id_mapping(clone_id, id);
        self.state.stack.push(clone_id);
    }

    fn replace_node_with(&mut self, id: ElementId, m: usize) {
        let new_nodes = self.state.stack.split_off(self.state.stack.len() - m);
        let anchor_node_id = self.state.element_to_node_id(id);
        self.doc.insert_before(anchor_node_id, &new_nodes);
        self.doc.remove(anchor_node_id);
    }

    fn replace_placeholder_with_nodes(&mut self, path: &'static [u8], m: usize) {
        let new_nodes = self.state.stack.split_off(self.state.stack.len() - m);
        let anchor_node_id = self.state.load_child(self.rdom, path);
        self.doc.insert_before(anchor_node_id, &new_nodes);
        self.doc.remove(anchor_node_id);
    }

    fn insert_nodes_after(&mut self, id: ElementId, m: usize) {
        let new_nodes = self.state.stack.split_off(self.state.stack.len() - m);
        let anchor_node_id = self.state.element_to_node_id(id);
        let next_sibling_id = self
            .doc
            .get_node(anchor_node_id)
            .unwrap()
            .forward(1)
            .map(|node| node.id);

        match next_sibling_id {
            Some(anchor_node_id) => {
                self.doc.insert_before(anchor_node_id, &new_nodes);
            }
            None => self.doc.append(anchor_node_id, &new_nodes),
        }
    }

    fn insert_nodes_before(&mut self, id: ElementId, m: usize) {
        let new_nodes = self.state.stack.split_off(self.state.stack.len() - m);
        let anchor_node_id = self.state.element_to_node_id(id);
        self.doc.insert_before(anchor_node_id, &new_nodes);
    }

    fn set_attribute(
        &mut self,
        name: &'static str,
        ns: Option<&'static str>,
        value: &AttributeValue,
        id: ElementId,
    ) {
        let node_id = self.state.element_to_node_id(id);
        let mut node = self.rdom.get_mut(node_id).unwrap();
        let mut node_type_mut = node.node_type_mut();
        if let NodeData::Element(ref mut element) = node.raw_dom_data {
            // FIXME: support non-text attributes
            if let AttributeValue::Text(val) = value {
                // FIXME check namespace
                let existing_attr = element.attrs.iter_mut().find(|attr| attr.name == name)?;
                if let Some(ref mut existing_attr) = existing_attr {
                    existing_attr.value = val;
                } else {
                    element.attrs.push(Attribute {
                        name: qual_name(name, ns),
                        value: val,
                    });
                }
            }

            if let AttributeValue::None = value {
                // FIXME: check namespace
                element.attrs.retain(|attr| attr.name != name);
            }
        }
    }

    fn set_node_text(&mut self, value: &str, id: ElementId) {
        let node_id = self.state.element_to_node_id(id);
        let mut node = self.doc.get_node_mut(node_id).unwrap();
        if let NodeData::Text(ref mut text) = node.raw_dom_data {
            text.content = value.to_string();
        }
    }

    fn create_event_listener(&mut self, name: &'static str, id: ElementId) {
        // let node_id = self.state.element_to_node_id(id);
        // let mut node = self.rdom.get_mut(node_id).unwrap();
        // node.add_event_listener(name);
    }

    fn remove_event_listener(&mut self, name: &'static str, id: ElementId) {
        // let node_id = self.state.element_to_node_id(id);
        // let mut node = self.rdom.get_mut(node_id).unwrap();
        // node.remove_event_listener(name);
    }

    fn remove_node(&mut self, id: ElementId) {
        let node_id = self.state.element_to_node_id(id);
        self.doc.remove_node(node_id);
    }

    fn push_root(&mut self, id: ElementId) {
        let node_id = self.state.element_to_node_id(id);
        self.state.stack.push(node_id);
    }
}

fn create_template_node(doc: &mut Document, node: &TemplateNode) -> NodeId {
    match node {
        TemplateNode::Element {
            tag,
            namespace,
            attrs,
            children,
        } => {
            let id_attr_atom = attrs.iter().find_map(|attr| match attr {
                TemplateAttribute::Static { name, .. } if *name == "id" => Some(Atom::from(*name)),
                _ => None,
            });
            let mut data = ElementNodeData {
                name: qual_name(*tag, *namespace),
                id: id_attr_atom,
                attrs: attrs
                    .into_iter()
                    .filter_map(|attr| match attr {
                        TemplateAttribute::Static {
                            name,
                            value,
                            namespace,
                        } => Some(Attribute {
                            name: qual_name(*name, *namespace),
                            value: value.to_string(),
                        }),
                        TemplateAttribute::Dynamic { .. } => None,
                    })
                    .collect(),
                style_attribute: Default::default(),
                image: None,
                template_contents: None,
                // listeners: FxHashSet::default(),
            };
            data.flush_style_attribute(doc.guard());

            let id = doc.create_node(NodeData::Element(data));
            let node = doc.get_node(id).unwrap();

            // Initialise style data
            *node.stylo_element_data.borrow_mut() = Some(Default::default());

            // If the node has an "id" attribute, store it in the ID map.
            // FIXME: implement
            // if let Some(id_attr) = node.attr(local_name!("id")) {
            //     doc.nodes_to_id.insert(id_attr.to_string(), id);
            // }

            let child_ids: Vec<NodeId> = children
                .into_iter()
                .map(|child| create_template_node(doc, child))
                .collect();
            doc.get_node_mut(id).unwrap().children = child_ids;

            id
        }
        TemplateNode::Text { text } => doc.create_text_node(text),
        TemplateNode::Dynamic { .. } => doc.create_node(NodeData::Comment),
        TemplateNode::DynamicText { .. } => doc.create_text_node(""),
    }
}
