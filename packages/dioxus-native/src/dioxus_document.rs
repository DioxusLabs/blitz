//! Integration between Dioxus and Blitz

use std::ops::{Deref, DerefMut};
use std::{any::Any, collections::HashMap, rc::Rc, sync::Arc};

use blitz_dom::{
    Atom, BaseDocument, DEFAULT_CSS, Document, ElementNodeData, Node, NodeData, QualName,
    net::Resource, ns,
};
use blitz_dom::{EventDriver, EventHandler};

use blitz_traits::EventState;
use blitz_traits::events::UiEvent;
use blitz_traits::{ColorScheme, DomEvent, DomEventData, Viewport, net::NetProvider};
use dioxus_core::{ElementId, Event, VirtualDom};
use dioxus_html::{PlatformEventData, set_event_converter};
use futures_util::{FutureExt, pin_mut};

use super::event_handler::{NativeClickData, NativeConverter, NativeFormData};
use crate::keyboard_event::BlitzKeyboardData;
use crate::mutation_writer::{DioxusState, MutationWriter};

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

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
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


