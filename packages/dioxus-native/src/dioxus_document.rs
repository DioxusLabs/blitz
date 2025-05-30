//! Integration between Dioxus and Blitz
use futures_util::{FutureExt, pin_mut};
use rustc_hash::FxHashMap;
use std::ops::{Deref, DerefMut};
use std::{any::Any, collections::HashMap, rc::Rc, sync::Arc};

use blitz_dom::{
    Atom, Attribute, BaseDocument, DEFAULT_CSS, Document, DocumentMutator, ElementNodeData,
    EventDriver, EventHandler, Node, NodeData, QualName, net::Resource, ns,
};
use blitz_traits::{
    ColorScheme, DomEvent, DomEventData, EventState, Viewport, events::UiEvent, net::NetProvider,
};

use dioxus_core::{
    AttributeValue, ElementId, Event, Template, TemplateAttribute, TemplateNode, VirtualDom,
    WriteMutations,
};
use dioxus_html::{PlatformEventData, set_event_converter};

use super::event_handler::{NativeClickData, NativeConverter, NativeFormData};
use crate::{NodeId, keyboard_event::BlitzKeyboardData, trace};

pub(crate) fn qual_name(local_name: &str, namespace: Option<&str>) -> QualName {
    QualName {
        prefix: None,
        ns: namespace.map(Atom::from).unwrap_or(ns!(html)),
        local: Atom::from(local_name),
    }
}

pub struct DioxusDocument {
    pub(crate) vdom: VirtualDom,
    vdom_state: DioxusState,
    inner: BaseDocument,
}

// Implement DocumentLike and required traits for DioxusDocument
impl Deref for DioxusDocument {
    type Target = BaseDocument;
    fn deref(&self) -> &BaseDocument {
        &self.inner
    }
}
impl DerefMut for DioxusDocument {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
impl From<DioxusDocument> for BaseDocument {
    fn from(doc: DioxusDocument) -> BaseDocument {
        doc.inner
    }
}
impl Document for DioxusDocument {
    fn id(&self) -> usize {
        self.inner.id()
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn poll(&mut self, mut cx: std::task::Context) -> bool {
        {
            let fut = self.vdom.wait_for_work();
            pin_mut!(fut);

            match fut.poll_unpin(&mut cx) {
                std::task::Poll::Ready(_) => {}
                std::task::Poll::Pending => return false,
            }
        }

        let mut writer = MutationWriter::new(&mut self.inner, &mut self.vdom_state);
        self.vdom.render_immediate(&mut writer);

        true
    }

    fn handle_event(&mut self, event: UiEvent) {
        set_event_converter(Box::new(NativeConverter {}));
        let handler = DioxusEventHandler {
            vdom: &mut self.vdom,
            vdom_state: &mut self.vdom_state,
        };
        let mut driver = EventDriver::new(self.inner.mutate(), handler);
        driver.handle_ui_event(event);
    }
}

pub struct DioxusEventHandler<'v> {
    vdom: &'v mut VirtualDom,
    #[allow(dead_code, reason = "WIP")]
    vdom_state: &'v mut DioxusState,
}

impl EventHandler for DioxusEventHandler<'_> {
    fn handle_event(
        &mut self,
        node_id: usize,
        event: &mut DomEvent,
        mutr: &mut blitz_dom::DocumentMutator<'_>,
        event_state: &mut EventState,
    ) {
        let dioxus_id = mutr.doc.get_node(node_id).and_then(get_dioxus_id);
        let Some(id) = dioxus_id else {
            return;
        };

        let event_data = match &event.data {
            DomEventData::MouseMove { .. }
            | DomEventData::MouseDown { .. }
            | DomEventData::MouseUp { .. }
            | DomEventData::Click(_) => Some(wrap_event_data(NativeClickData)),

            DomEventData::KeyDown(kevent)
            | DomEventData::KeyUp(kevent)
            | DomEventData::KeyPress(kevent) => {
                Some(wrap_event_data(BlitzKeyboardData(kevent.clone())))
            }

            DomEventData::Input(data) => Some(wrap_event_data(NativeFormData {
                value: data.value.clone(),
                values: HashMap::new(),
            })),

            // TODO: Implement IME handling
            DomEventData::Ime(_) => None,
        };

        let Some(event_data) = event_data else {
            return;
        };

        let dx_event = Event::new(event_data.clone(), event.bubbles);
        self.vdom
            .runtime()
            .handle_event(event.name(), dx_event.clone(), id);

        if !dx_event.default_action_enabled() {
            event_state.prevent_default();
        }
        if !dx_event.propagates() {
            event_state.stop_propagation()
        }
    }
}

fn wrap_event_data<T: Any>(value: T) -> Rc<dyn Any> {
    Rc::new(PlatformEventData::new(Box::new(value)))
}

fn get_dioxus_id(node: &Node) -> Option<ElementId> {
    node.element_data()?
        .attrs
        .iter()
        .find(|attr| *attr.name.local == *"data-dioxus-id")
        .and_then(|attr| attr.value.parse::<usize>().ok())
        .map(ElementId)
}

impl DioxusDocument {
    pub fn new(vdom: VirtualDom, net_provider: Option<Arc<dyn NetProvider<Resource>>>) -> Self {
        let viewport = Viewport::new(0, 0, 1.0, ColorScheme::Light);
        let mut doc = BaseDocument::new(viewport);

        // Set net provider
        if let Some(net_provider) = net_provider {
            doc.set_net_provider(net_provider);
        }

        // Create a virtual "html" element to act as the root element, as we won't necessarily
        // have a single root otherwise, while the rest of blitz requires that we do
        let html_element_id = doc.create_node(NodeData::Element(ElementNodeData::new(
            qual_name("html", None),
            Vec::new(),
        )));
        let root_node_id = doc.root_node().id;
        let html_element = doc.get_node_mut(html_element_id).unwrap();
        html_element.parent = Some(root_node_id);
        let root_node = doc.get_node_mut(root_node_id).unwrap();
        root_node.children.push(html_element_id);

        // Include default and user-specified stylesheets
        doc.add_user_agent_stylesheet(DEFAULT_CSS);

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
        let mut writer = MutationWriter::new(&mut self.inner, &mut self.vdom_state);
        self.vdom.rebuild(&mut writer);
        // dbg!(self.vdom.rebuild_to_vec());
        // std::process::exit(0);
        // dbg!(writer.state);
    }
}

/// The state of the Dioxus integration with the RealDom
#[derive(Debug)]
pub struct DioxusState {
    /// Store of templates keyed by unique name
    templates: FxHashMap<Template, Vec<NodeId>>,
    /// Stack machine state for applying dioxus mutations
    stack: Vec<NodeId>,
    /// Mapping from vdom ElementId -> rdom NodeId
    node_id_mapping: Vec<Option<NodeId>>,
}

impl DioxusState {
    /// Initialize the DioxusState in the RealDom
    pub fn create(doc: &mut BaseDocument) -> Self {
        let root = doc.root_element();
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

    fn anchor_and_nodes(&mut self, id: ElementId, m: usize) -> (usize, Vec<usize>) {
        let anchor_node_id = self.element_to_node_id(id);
        let new_nodes = self.m_stack_nodes(m);
        (anchor_node_id, new_nodes)
    }

    fn m_stack_nodes(&mut self, m: usize) -> Vec<usize> {
        self.stack.split_off(self.stack.len() - m)
    }
}

/// A writer for mutations that can be used with the RealDom.
pub struct MutationWriter<'a> {
    /// The realdom associated with this writer
    pub docm: DocumentMutator<'a>,
    /// The state associated with this writer
    pub state: &'a mut DioxusState,
}

impl<'a> MutationWriter<'a> {
    pub fn new(doc: &'a mut BaseDocument, state: &'a mut DioxusState) -> Self {
        MutationWriter {
            docm: doc.mutate(),
            state,
        }
    }
}

impl MutationWriter<'_> {
    /// Update an ElementId -> NodeId mapping
    fn set_id_mapping(&mut self, node_id: NodeId, element_id: ElementId) {
        let element_id: usize = element_id.0;

        // Ensure node_id_mapping is large enough to contain element_id
        if self.state.node_id_mapping.len() <= element_id {
            self.state.node_id_mapping.resize(element_id + 1, None);
        }

        // Set the new mapping
        self.state.node_id_mapping[element_id] = Some(node_id);
    }

    /// Create a ElementId -> NodeId mapping and push the node to the stack
    fn map_new_node(&mut self, node_id: NodeId, element_id: ElementId) {
        self.set_id_mapping(node_id, element_id);
        self.state.stack.push(node_id);
    }

    /// Find a child in the document by child index path
    fn load_child(&self, path: &[u8]) -> NodeId {
        let top_of_stack_node_id = *self.state.stack.last().unwrap();
        self.docm.node_at_path(top_of_stack_node_id, path)
    }
}

impl WriteMutations for MutationWriter<'_> {
    fn assign_node_id(&mut self, path: &'static [u8], id: ElementId) {
        trace!("assign_node_id path:{:?} id:{}", path, id.0);

        // If there is an existing node already mapped to that ID and it has no parent, then drop it
        // TODO: more automated GC/ref-counted semantics for node lifetimes
        if let Some(node_id) = self.state.try_element_to_node_id(id) {
            self.docm.remove_node_if_unparented(node_id);
        }

        // Map the node at specified path
        self.set_id_mapping(self.load_child(path), id);
    }

    fn create_placeholder(&mut self, id: ElementId) {
        trace!("create_placeholder id:{}", id.0);
        let node_id = self.docm.create_comment_node();
        self.map_new_node(node_id, id);
    }

    fn create_text_node(&mut self, value: &str, id: ElementId) {
        trace!("create_text_node id:{} text:{}", id.0, value);
        let node_id = self.docm.create_text_node(value);
        self.map_new_node(node_id, id);
    }

    fn append_children(&mut self, id: ElementId, m: usize) {
        trace!("append_children id:{} m:{}", id.0, m);
        let (parent_id, child_node_ids) = self.state.anchor_and_nodes(id, m);
        self.docm.append_children(parent_id, &child_node_ids);
    }

    fn insert_nodes_after(&mut self, id: ElementId, m: usize) {
        trace!("insert_nodes_after id:{} m:{}", id.0, m);
        let (anchor_node_id, new_node_ids) = self.state.anchor_and_nodes(id, m);
        self.docm.insert_nodes_after(anchor_node_id, &new_node_ids);
    }

    fn insert_nodes_before(&mut self, id: ElementId, m: usize) {
        trace!("insert_nodes_before id:{} m:{}", id.0, m);
        let (anchor_node_id, new_node_ids) = self.state.anchor_and_nodes(id, m);
        self.docm.insert_nodes_before(anchor_node_id, &new_node_ids);
    }

    fn replace_node_with(&mut self, id: ElementId, m: usize) {
        trace!("replace_node_with id:{} m:{}", id.0, m);
        let (anchor_node_id, new_node_ids) = self.state.anchor_and_nodes(id, m);
        self.docm.replace_node_with(anchor_node_id, &new_node_ids);
    }

    fn replace_placeholder_with_nodes(&mut self, path: &'static [u8], m: usize) {
        trace!("replace_placeholder_with_nodes path:{:?} m:{}", path, m);
        // WARNING: DO NOT REORDER
        // The order of the following two lines is very important as "m_stack_nodes" mutates
        // the stack and then "load_child" reads from the top of the stack.
        let new_node_ids = self.state.m_stack_nodes(m);
        let anchor_node_id = self.load_child(path);
        self.docm
            .replace_placeholder_with_nodes(anchor_node_id, &new_node_ids);
    }

    fn remove_node(&mut self, id: ElementId) {
        trace!("remove_node id:{}", id.0);
        let node_id = self.state.element_to_node_id(id);
        self.docm.remove_node(node_id);
    }

    fn push_root(&mut self, id: ElementId) {
        trace!("push_root id:{}", id.0);
        let node_id = self.state.element_to_node_id(id);
        self.state.stack.push(node_id);
    }

    fn set_node_text(&mut self, value: &str, id: ElementId) {
        trace!("set_node_text id:{} value:{}", id.0, value);
        let node_id = self.state.element_to_node_id(id);
        self.docm.set_node_text(node_id, value);
    }

    fn set_attribute(
        &mut self,
        local_name: &'static str,
        ns: Option<&'static str>,
        value: &AttributeValue,
        id: ElementId,
    ) {
        let node_id = self.state.element_to_node_id(id);
        trace!("set_attribute node_id:{node_id} ns: {ns:?} name:{local_name}, value:{value:?}");

        // Dioxus has overloaded the style namespace to accumulate style attributes without a `style` block
        // TODO: accumulate style attributes into a single style element.
        if ns == Some("style") {
            return;
        }

        let name = qual_name(local_name, ns);
        match value {
            AttributeValue::Text(value) => {
                self.docm.set_attribute(node_id, name, value);
            }
            AttributeValue::Float(value) => {
                let value = value.to_string();
                self.docm.set_attribute(node_id, name, &value);
            }
            AttributeValue::Int(value) => {
                let value = value.to_string();
                self.docm.set_attribute(node_id, name, &value);
            }
            AttributeValue::None => {
                self.docm.clear_attribute(node_id, name);
            }
            _ => { /* FIXME: support all attribute types */ }
        }
    }

    fn load_template(&mut self, template: Template, index: usize, id: ElementId) {
        // TODO: proper template node support
        let template_entry = self.state.templates.entry(template).or_insert_with(|| {
            let template_root_ids: Vec<NodeId> = template
                .roots
                .iter()
                .map(|root| create_template_node(&mut self.docm, root))
                .collect();

            template_root_ids
        });

        let template_node_id = template_entry[index];
        let clone_id = self.docm.deep_clone_node(template_node_id);

        trace!("load_template template_node_id:{template_node_id} clone_id:{clone_id}");
        self.map_new_node(clone_id, id);
    }

    fn create_event_listener(&mut self, name: &'static str, id: ElementId) {
        // We're going to actually set the listener here as a placeholder - in JS this would also be a placeholder
        // we might actually just want to attach the attribute to the root element (delegation)
        let value = AttributeValue::Text("<rust func>".into());
        self.set_attribute(name, None, &value, id);

        // Also set the data-dioxus-id attribute so we can find the element later
        let value = AttributeValue::Text(id.0.to_string());
        self.set_attribute("data-dioxus-id", None, &value, id);

        // node.add_event_listener(name);
    }

    fn remove_event_listener(&mut self, _name: &'static str, _id: ElementId) {
        // node.remove_event_listener(name);
    }
}

fn create_template_node(docm: &mut DocumentMutator<'_>, node: &TemplateNode) -> NodeId {
    match node {
        TemplateNode::Element {
            tag,
            namespace,
            attrs,
            children,
        } => {
            let name = qual_name(tag, *namespace);
            let attrs = attrs.iter().filter_map(map_template_attr).collect();
            let node_id = docm.create_element(name, attrs);

            let child_ids: Vec<NodeId> = children
                .iter()
                .map(|child| create_template_node(docm, child))
                .collect();

            docm.append_children(node_id, &child_ids);

            node_id
        }
        TemplateNode::Text { text } => docm.create_text_node(text),
        TemplateNode::Dynamic { .. } => docm.create_comment_node(),
    }
}

fn map_template_attr(attr: &TemplateAttribute) -> Option<Attribute> {
    let TemplateAttribute::Static {
        name,
        value,
        namespace,
    } = attr
    else {
        return None;
    };

    let name = qual_name(name, *namespace);
    let value = value.to_string();
    Some(Attribute { name, value })
}
