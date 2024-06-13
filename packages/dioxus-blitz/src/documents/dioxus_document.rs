//! Integration between Dioxus and Blitz

use std::rc::Rc;

use blitz::Viewport;
use blitz_dom::{
    namespace_url, node::Attribute, ns, Atom, Document, DocumentLike, ElementNodeData, NodeData,
    QualName, TextNodeData,
};

use dioxus::{
    dioxus_core::{
        AttributeValue, ElementId, Template, TemplateAttribute, TemplateNode, VirtualDom,
        WriteMutations,
    },
    prelude::{set_event_converter, PlatformEventData},
};
use futures_util::{pin_mut, FutureExt};
use rustc_hash::FxHashMap;

use super::event_handler::{NativeClickData, NativeConverter};

type NodeId = usize;

fn qual_name(local_name: &str, namespace: Option<&str>) -> QualName {
    QualName {
        prefix: None,
        ns: namespace.map(Atom::from).unwrap_or(ns!(html)),
        local: Atom::from(local_name),
    }
}

pub struct DioxusDocument {
    vdom: VirtualDom,
    vdom_state: DioxusState,
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
impl DocumentLike for DioxusDocument {
    fn poll(&mut self, mut cx: std::task::Context) -> bool {
        loop {
            {
                let fut = self.vdom.wait_for_work();
                pin_mut!(fut);

                match fut.poll_unpin(&mut cx) {
                    std::task::Poll::Ready(_) => {}
                    std::task::Poll::Pending => return false,
                }
            }

            self.vdom.render_immediate(&mut MutationWriter {
                doc: &mut self.inner,
                state: &mut self.vdom_state,
            });
            return true;
        }
    }

    fn handle_event(&mut self, event: blitz_dom::events::RendererEvent) -> bool {
        // Collect the nodes into a chain by traversing upwards
        // This is important so the "capture" phase can be implemented
        let mut node_id = event.target;
        let mut chain = Vec::with_capacity(16);
        chain.push(node_id);

        // if it's a capturing event, we want to fill in the chain with the parent nodes
        // until we reach the root - that way we can call the listeners in the correct order
        // otherwise, we just want to call the listeners on the target
        //
        // todo: this is harcoded for "click" events - eventually we actually need to handle proper propagation
        // if event.name == "click" {
        while let Some(parent) = self.inner.tree()[node_id].parent {
            chain.push(parent);
            node_id = parent;
        }

        set_event_converter(Box::new(NativeConverter {}));

        // look for the data-dioxus-id attribute on the element
        // todo: we might need to walk upwards to find the first element with a data-dioxus-id attribute
        for node in chain.iter() {
            let Some(element) = self.inner.tree()[*node].element_data() else {
                println!(
                    "No element data found for node {}: {:?}",
                    node,
                    self.inner.tree()[*node]
                );
                continue;
            };

            for attr in element.attrs() {
                if attr.name.local.as_ref() == "data-dioxus-id" {
                    if let Ok(value) = attr.value.parse::<usize>() {
                        let id = ElementId(value);
                        // let data = dioxus::html::EventData::Mouse()

                        let data = Rc::new(PlatformEventData::new(Box::new(NativeClickData {})));
                        self.vdom.handle_event("click", data, id, true);
                        return true;
                    }
                }
            }
        }

        false
    }
}

impl DioxusDocument {
    pub fn new(vdom: VirtualDom) -> Self {
        let device = Viewport::new((0, 0)).make_device();
        let mut doc = Document::new(device);

        // doc.add_element()

        // Include default and user-specified stylesheets
        doc.add_stylesheet(include_str!("./default.css"));

        let state = DioxusState::create(&mut doc);
        let mut doc = Self {
            vdom,
            vdom_state: state,
            inner: doc,
        };

        doc.initial_build();

        doc.inner.print_tree();

        doc
    }

    pub fn initial_build(&mut self) {
        let mut writer = MutationWriter {
            doc: &mut self.inner,
            state: &mut self.vdom_state,
        };
        self.vdom.rebuild(&mut writer);
        // dbg!(self.vdom.rebuild_to_vec());
        // std::process::exit(0);
        // dbg!(writer.state);
    }

    // pub fn apply_mutations(&mut self) {
    //     // Apply the mutations to the actual dom
    //     let mut writer = MutationWriter {
    //         doc: &mut self.inner,
    //         state: &mut self.vdom_state,
    //     };
    //     self.vdom.render_immediate(&mut writer);

    //     println!("APPLY MUTATIONS");
    //     self.inner.print_tree();
    // }
}

/// The state of the Dioxus integration with the RealDom
#[derive(Debug)]
pub struct DioxusState {
    /// Store of templates keyed by unique name
    templates: FxHashMap<String, Vec<NodeId>>,
    /// Stack machine state for applying dioxus mutations
    stack: Vec<NodeId>,
    /// Mapping from vdom ElementId -> rdom NodeId
    node_id_mapping: Vec<Option<NodeId>>,
}

/// A writer for mutations that can be used with the RealDom.
pub struct MutationWriter<'a> {
    /// The realdom associated with this writer
    pub doc: &'a mut Document,

    /// The state associated with this writer
    pub state: &'a mut DioxusState,
}

impl DioxusState {
    /// Initialize the DioxusState in the RealDom
    pub fn create(doc: &mut Document) -> Self {
        let root = doc.root_node();
        let root_id = root.id;
        dbg!(Self {
            templates: FxHashMap::default(),
            stack: vec![root_id],
            node_id_mapping: vec![Some(root_id)],
        })
    }

    /// Convert an ElementId to a NodeId
    pub fn element_to_node_id(&self, element_id: ElementId) -> NodeId {
        self.try_element_to_node_id(element_id).unwrap()
    }

    /// Attempt to convert an ElementId to a NodeId. This will return None if the ElementId is not in the RealDom.
    pub fn try_element_to_node_id(&self, element_id: ElementId) -> Option<NodeId> {
        self.node_id_mapping.get(element_id.0).copied().flatten()
    }

    // /// Create a mutation writer for the RealDom
    // pub fn create_mutation_writer<'a>(&'a mut self, doc: &'a mut Document) -> MutationWriter<'a> {
    //     MutationWriter { doc, state: self }
    // }
}

impl MutationWriter<'_> {
    /// Update an ElementId -> NodeId mapping
    fn set_id_mapping(&mut self, node_id: NodeId, element_id: ElementId) {
        let element_id: usize = element_id.0;

        // Ensure node_id_mapping is large enough to contain element_id
        if self.state.node_id_mapping.len() <= element_id {
            self.state.node_id_mapping.resize(element_id + 1, None);
        }
        // If element_id is already mapping to a node, remove that node from the document
        else if let Some(mapped_node_id) = self.state.node_id_mapping[element_id] {
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

impl WriteMutations for MutationWriter<'_> {
    fn register_template(&mut self, template: Template) {
        println!("register_template name:{}", template.name);
        let template_root_ids: Vec<NodeId> = template
            .roots
            .iter()
            .map(|root| create_template_node(self.doc, root))
            .collect();
        dbg!(&template_root_ids);
        self.state
            .templates
            .insert(template.name.to_string(), template_root_ids);
    }

    fn append_children(&mut self, id: ElementId, m: usize) {
        println!("append_children id:{} m:{}", id.0, m);
        let children = self.state.stack.split_off(self.state.stack.len() - m);
        let parent = self.state.element_to_node_id(id);
        for child in children {
            self.doc.get_node_mut(parent).unwrap().children.push(child);
        }
    }

    fn assign_node_id(&mut self, path: &'static [u8], id: ElementId) {
        println!("assign_node_id path:{:?} id:{}", path, id.0);
        let node_id = self.load_child(path);
        self.set_id_mapping(node_id, id);
    }

    fn create_placeholder(&mut self, id: ElementId) {
        println!("create_placeholder id:{}", id.0);
        let node_id = self.doc.create_node(NodeData::Comment);
        self.set_id_mapping(node_id, id);
        self.state.stack.push(node_id);
    }

    fn create_text_node(&mut self, value: &str, id: ElementId) {
        println!("create_text_node id:{} text:{}", id.0, value);
        let node_id = self.doc.create_text_node(value);
        self.set_id_mapping(node_id, id);
        self.state.stack.push(node_id);
    }

    fn hydrate_text_node(&mut self, path: &'static [u8], value: &str, id: ElementId) {
        let value_trunc = if value.len() > 20 {
            &value[0..20]
        } else {
            value
        };
        println!(
            "hydrate_text_node id:{} path: {:?} text:{}",
            id.0, path, value_trunc
        );
        let node_id = self.load_child(path);
        self.set_id_mapping(node_id, id);
        let node = self.doc.get_node_mut(node_id).unwrap();
        if let NodeData::Text(ref mut text) = node.raw_dom_data {
            text.content = value.to_string();
        } else {
            node.raw_dom_data = NodeData::Text(TextNodeData::new(value.to_string()));
        }

        let parent = node.parent;

        if let Some(parent) = parent {
            // if the text is the child of a style element, we want to put the style into the stylesheet cache
            let parent = self.doc.get_node(parent).unwrap();
            if let NodeData::Element(ref element) = parent.raw_dom_data {
                if element.name.local.as_ref() == "style" {
                    self.doc.add_stylesheet(value);
                }
            }
        }
    }

    fn load_template(&mut self, name: &'static str, index: usize, id: ElementId) {
        println!("load_template name:{} index: {} id:{}", name, index, id.0);
        let template_node_id = self.state.templates[name][index];
        let clone_id = self.doc.deep_clone_node(template_node_id);
        self.set_id_mapping(clone_id, id);
        self.state.stack.push(clone_id);
    }

    fn replace_node_with(&mut self, id: ElementId, m: usize) {
        println!("replace_node_with id:{} m:{}", id.0, m);
        let new_nodes = self.state.stack.split_off(self.state.stack.len() - m);
        let anchor_node_id = self.state.element_to_node_id(id);
        self.doc.insert_before(anchor_node_id, &new_nodes);
        self.doc.remove_node(anchor_node_id);
    }

    fn replace_placeholder_with_nodes(&mut self, path: &'static [u8], m: usize) {
        println!("replace_placeholder_with_nodes path:{:?} m:{}", path, m);
        let new_nodes = self.state.stack.split_off(self.state.stack.len() - m);
        let anchor_node_id = self.load_child(path);
        self.doc.insert_before(anchor_node_id, &new_nodes);
        self.doc.remove_node(anchor_node_id);
    }

    fn insert_nodes_after(&mut self, id: ElementId, m: usize) {
        println!("insert_nodes_after id:{} m:{}", id.0, m);
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
        println!("insert_nodes_before id:{} m:{}", id.0, m);
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
        let node = self.doc.get_node_mut(node_id).unwrap();
        if let NodeData::Element(ref mut element) = node.raw_dom_data {
            // FIXME: support non-text attributes
            if let AttributeValue::Text(val) = value {
                // FIXME check namespace
                let existing_attr = element
                    .attrs
                    .iter_mut()
                    .find(|attr| attr.name.local == *name);

                if let Some(existing_attr) = existing_attr {
                    existing_attr.value = val.to_string();
                } else {
                    // we have overloaded the style namespace to accumulate style attributes without a `style` block
                    if ns == Some("style") {
                        // todo: need to accumulate style attributes into a single style
                        //
                        // element.
                    } else {
                        element.attrs.push(Attribute {
                            name: qual_name(name, ns),
                            value: val.to_string(),
                        });
                    }
                }
            }

            if let AttributeValue::None = value {
                // FIXME: check namespace
                element.attrs.retain(|attr| attr.name.local != *name);
            }
        }
    }

    fn set_node_text(&mut self, value: &str, id: ElementId) {
        let node_id = self.state.element_to_node_id(id);
        let node = self.doc.get_node_mut(node_id).unwrap();

        let text = match node.raw_dom_data {
            NodeData::Text(ref mut text) => text,

            // todo: otherwise this is basically element.textContent which is a bit different - need to parse as html
            _ => return,
        };

        // todo: this is very inefficient for inline styles - lots of string cloning going on
        let changed = text.content != value;
        text.content = value.to_string();
        let contents = text.content.clone();

        if let Some(parent) = node.parent {
            // if the text is the child of a style element, we want to put the style into the stylesheet cache
            let parent = self.doc.get_node(parent).unwrap();
            if let NodeData::Element(ref element) = parent.raw_dom_data {
                // Only set stylsheets if the text content has *changed* - we need to unload
                if changed && element.name.local.as_ref() == "style" {
                    self.doc.add_stylesheet(value);
                    self.doc.remove_stylehsheet(&contents);
                }
            }
        }
    }

    fn create_event_listener(&mut self, _name: &'static str, _id: ElementId) {
        // we're going to actually set the listener here as a placeholder - in JS this would also be a placeholder
        // we might actually just want to attach the attribute to the root element (delegation)
        self.set_attribute(
            _name,
            None,
            &AttributeValue::Text("<rust func>".into()),
            _id,
        );

        // also set the data-dioxus-id attribute so we can find the element later
        self.set_attribute(
            "data-dioxus-id",
            None,
            &AttributeValue::Text(_id.0.to_string()),
            _id,
        );

        // let node_id = self.state.element_to_node_id(id);
        // let mut node = self.rdom.get_mut(node_id).unwrap();
        // node.add_event_listener(name);
    }

    fn remove_event_listener(&mut self, _name: &'static str, _id: ElementId) {
        // let node_id = self.state.element_to_node_id(id);
        // let mut node = self.rdom.get_mut(node_id).unwrap();
        // node.remove_event_listener(name);
    }

    fn remove_node(&mut self, id: ElementId) {
        println!("remove_node id:{}", id.0);
        let node_id = self.state.element_to_node_id(id);
        self.doc.remove_node(node_id);
    }

    fn push_root(&mut self, id: ElementId) {
        println!("push_root id:{}", id.0,);
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
            let name = qual_name(tag, *namespace);
            let attrs = attrs
                .iter()
                .filter_map(|attr| match attr {
                    TemplateAttribute::Static {
                        name,
                        value,
                        namespace,
                    } => Some(Attribute {
                        name: qual_name(name, *namespace),
                        value: value.to_string(),
                    }),
                    TemplateAttribute::Dynamic { .. } => None,
                })
                .collect();

            let mut data = ElementNodeData::new(name, attrs);
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
                .iter()
                .map(|child| create_template_node(doc, child))
                .collect();
            for &child_id in &child_ids {
                doc.get_node_mut(child_id).unwrap().parent = Some(id);
            }
            doc.get_node_mut(id).unwrap().children = child_ids;

            id
        }
        TemplateNode::Text { text } => doc.create_text_node(text),
        TemplateNode::Dynamic { .. } => doc.create_node(NodeData::Comment),
        TemplateNode::DynamicText { .. } => doc.create_text_node(""),
    }
}
