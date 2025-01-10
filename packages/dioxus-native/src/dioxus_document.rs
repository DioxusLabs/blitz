//! Integration between Dioxus and Blitz

use std::{any::Any, collections::HashMap, rc::Rc, sync::Arc};

use blitz_dom::{
    local_name, namespace_url, net::Resource, node::NodeSpecificData, ns, Atom, BaseDocument,
    ElementNodeData, Node, NodeData, QualName, DEFAULT_CSS,
};

use blitz_traits::{net::NetProvider, ColorScheme, Document, DomEvent, DomEventData, Viewport};
use dioxus_core::{ElementId, Event, VirtualDom};
use dioxus_html::{set_event_converter, FormValue, PlatformEventData};
use futures_util::{pin_mut, FutureExt};

use super::event_handler::{NativeClickData, NativeConverter, NativeFormData};
use crate::keyboard_event::BlitzKeyboardData;
use crate::mutation_writer::{DioxusState, MutationWriter};
use crate::NodeId;

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

impl AsRef<BaseDocument> for DioxusDocument {
    fn as_ref(&self) -> &BaseDocument {
        &self.inner
    }
}
impl AsMut<BaseDocument> for DioxusDocument {
    fn as_mut(&mut self) -> &mut BaseDocument {
        &mut self.inner
    }
}
impl From<DioxusDocument> for BaseDocument {
    fn from(doc: DioxusDocument) -> BaseDocument {
        doc.inner
    }
}
impl Document for DioxusDocument {
    type Doc = BaseDocument;

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

    fn id(&self) -> usize {
        self.inner.id()
    }

    fn handle_event(&mut self, event: &mut DomEvent) {
        set_event_converter(Box::new(NativeConverter {}));

        let node = &self.inner.tree()[event.target];
        let Some(dioxus_id) = node.element_data().and_then(DioxusDocument::dioxus_id) else {
            return;
        };

        let dioxus_event = match &event.data {
            DomEventData::Event(event_type) => match event_type {
                &"input" => {
                    let element_data = self
                        .inner
                        .get_node(event.target)
                        .unwrap()
                        .element_data()
                        .unwrap();
                    let form_data = wrap_event_data(
                        self.input_event_form_data(event.composed_path(), element_data),
                    );
                    let event = Event::new(form_data, true);
                    Some(("input", event))
                }
                _ => None,
            },
            DomEventData::MouseDown { .. } => {
                let event_data = wrap_event_data(NativeClickData);
                let event = Event::new(event_data, true);
                Some(("mousedown", event))
            }
            DomEventData::MouseUp { .. } => {
                let event_data = wrap_event_data(NativeClickData);
                let event = Event::new(event_data, true);
                Some(("mouseup", event))
            }
            DomEventData::Click { .. } => {
                let event_data = wrap_event_data(NativeClickData);
                let event = Event::new(event_data.clone(), true);
                Some(("click", event))
            }
            DomEventData::Focus => None,
            DomEventData::Input(_) => {
                let element_data = self
                    .inner
                    .get_node(event.target)
                    .unwrap()
                    .element_data()
                    .unwrap();
                let form_data = wrap_event_data(
                    self.input_event_form_data(event.composed_path(), element_data),
                );
                let event = Event::new(form_data, true);
                Some(("input", event))
            }
            DomEventData::KeyDown(kevent) => {
                let event_data = wrap_event_data(BlitzKeyboardData(kevent.clone()));
                let event = Event::new(event_data.clone(), true);
                Some(("keydown", event))
            }
            DomEventData::KeyUp(kevent) => {
                let event_data = wrap_event_data(BlitzKeyboardData(kevent.clone()));
                let event = Event::new(event_data.clone(), true);
                Some(("keyup", event))
            }
            DomEventData::KeyPress(kevent) => {
                let event_data = wrap_event_data(BlitzKeyboardData(kevent.clone()));
                let event = Event::new(event_data.clone(), true);
                Some(("keypress", event))
            }
            // TODO: Implement IME and Hover events handling
            DomEventData::Ime(_) => None,
            DomEventData::Hover => None,
        };

        if let Some((name, dioxus_event)) = dioxus_event {
            self.vdom
                .runtime()
                .handle_event(name, dioxus_event.clone(), dioxus_id);

            if !dioxus_event.default_action_enabled() {
                event.prevent_default();
            }
            if !dioxus_event.propagates() {
                event.stop_propagation();
            }
        }
    }
}

fn wrap_event_data<T: Any>(value: T) -> Rc<dyn Any> {
    Rc::new(PlatformEventData::new(Box::new(value)))
}

impl DioxusDocument {
    /// Generate the FormData from an input event
    /// Currently only cares about input checkboxes
    pub fn input_event_form_data(
        &self,
        parent_chain: &[usize],
        element_node_data: &ElementNodeData,
    ) -> NativeFormData {
        let parent_form = parent_chain.iter().find_map(|id| {
            let node = self.inner.get_node(*id)?;
            let element_data = node.element_data()?;
            if element_data.name.local == local_name!("form") {
                Some(node)
            } else {
                None
            }
        });
        let values = if let Some(parent_form) = parent_form {
            let mut values = HashMap::<String, FormValue>::new();
            for form_input in self.input_descendents(parent_form).into_iter() {
                // Match html behaviour here. To be included in values:
                // - input must have a name
                // - if its an input, we only include it if checked
                // - if value is not specified, it defaults to 'on'
                if let Some(name) = form_input.attr(local_name!("name")) {
                    if form_input.attr(local_name!("type")) == Some("checkbox")
                        && form_input
                            .element_data()
                            .and_then(|data| data.checkbox_input_checked())
                            .unwrap_or(false)
                    {
                        let value = form_input
                            .attr(local_name!("value"))
                            .unwrap_or("on")
                            .to_string();
                        values.insert(name.to_string(), FormValue(vec![value]));
                    }
                }
            }
            values
        } else {
            Default::default()
        };
        let value = match &element_node_data.node_specific_data {
            NodeSpecificData::CheckboxInput(checked) => checked.to_string(),
            NodeSpecificData::TextInput(input_data) => input_data.editor.text().to_string(),
            _ => element_node_data
                .attr(local_name!("value"))
                .unwrap_or_default()
                .to_string(),
        };

        NativeFormData { value, values }
    }

    /// Collect all the inputs which are descendents of a given node
    fn input_descendents(&self, node: &Node) -> Vec<&Node> {
        node.children
            .iter()
            .flat_map(|id| {
                let mut res = Vec::<&Node>::new();
                let Some(n) = self.inner.get_node(*id) else {
                    return res;
                };
                let Some(element_data) = n.element_data() else {
                    return res;
                };
                if element_data.name.local == local_name!("input") {
                    res.push(n);
                }
                res.extend(self.input_descendents(n).iter());
                res
            })
            .collect()
    }

    pub fn new(
        vdom: VirtualDom,
        net_provider: Option<Arc<dyn NetProvider<Data = Resource>>>,
    ) -> Self {
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

    pub fn label_bound_input_element(&self, label_node_id: NodeId) -> Option<(ElementId, NodeId)> {
        let bound_input_elements = self.inner.label_bound_input_elements(label_node_id);

        // Filter down bound elements to those which have dioxus id
        bound_input_elements.into_iter().find_map(|n| {
            let target_element_data = n.element_data()?;
            let node_id = n.id;
            let dioxus_id = DioxusDocument::dioxus_id(target_element_data)?;
            Some((dioxus_id, node_id))
        })
    }

    fn dioxus_id(element_node_data: &ElementNodeData) -> Option<ElementId> {
        Some(ElementId(
            element_node_data
                .attrs
                .iter()
                .find(|attr| *attr.name.local == *"data-dioxus-id")?
                .value
                .parse::<usize>()
                .ok()?,
        ))
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
