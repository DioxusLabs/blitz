//! Integration between Dioxus and Blitz
use blitz_dom::DocumentConfig;
use futures_util::{FutureExt, pin_mut, task::noop_waker};
use std::ops::{Deref, DerefMut};
use std::sync::LazyLock;
use std::task::{Context as TaskContext, Waker};
use std::{any::Any, rc::Rc, sync::Arc};

use blitz_dom::{
    BaseDocument, DEFAULT_CSS, Document, EventDriver, EventHandler, Node, net::Resource,
};
use blitz_traits::{
    events::{DomEvent, DomEventData, EventState, UiEvent},
    net::NetProvider,
};

use dioxus_core::{ElementId, Event, VirtualDom};
use dioxus_html::{PlatformEventData, set_event_converter};

use crate::events::{BlitzKeyboardData, NativeClickData, NativeConverter, NativeFormData};
use crate::mutation_writer::{DioxusState, MutationWriter};
use crate::qual_name;

fn wrap_event_data<T: Any>(value: T) -> Rc<dyn Any> {
    Rc::new(PlatformEventData::new(Box::new(value)))
}

/// Get the value of the "dioxus-data-id" attribute parsed aa usize
fn get_dioxus_id(node: &Node) -> Option<ElementId> {
    node.element_data()?
        .attrs
        .iter()
        .find(|attr| *attr.name.local == *"data-dioxus-id")
        .and_then(|attr| attr.value.parse::<usize>().ok())
        .map(ElementId)
}

pub struct DioxusDocument {
    pub(crate) vdom: VirtualDom,
    vdom_state: DioxusState,
    inner: BaseDocument,
}

impl DioxusDocument {
    pub fn new(vdom: VirtualDom, net_provider: Option<Arc<dyn NetProvider<Resource>>>) -> Self {
        let mut doc = BaseDocument::new(DocumentConfig {
            net_provider,
            ..Default::default()
        });

        // Create some minimal HTML to render the app into.

        // Create the html element
        let mut mutr = doc.mutate();
        let html_element_id = mutr.create_element(qual_name("html", None), vec![]);
        mutr.append_children(mutr.doc.root_node().id, &[html_element_id]);

        // Create the head element
        let head_element_id = mutr.create_element(qual_name("head", None), vec![]);
        mutr.append_children(html_element_id, &[head_element_id]);

        // Create the body element
        let body_element_id = mutr.create_element(qual_name("body", None), vec![]);
        mutr.append_children(html_element_id, &[body_element_id]);

        // Create another virtual element to hold the root <div id="main"></div> under the html element
        let main_attr = blitz_dom::Attribute {
            name: qual_name("id", None),
            value: "main".to_string(),
        };
        let main_element_id = mutr.create_element(qual_name("main", None), vec![main_attr]);
        mutr.append_children(body_element_id, &[main_element_id]);

        drop(mutr);

        // Include default and user-specified stylesheets
        doc.add_user_agent_stylesheet(DEFAULT_CSS);

        let vdom_state = DioxusState::create(main_element_id);
        let mut doc = Self {
            vdom,
            vdom_state,
            inner: doc,
        };

        doc.inner.set_base_url("dioxus://index.html");
        doc.initial_build();

        #[cfg(feature = "tracing")]
        doc.inner.print_tree();

        doc
    }

    pub fn initial_build(&mut self) {
        let mut writer = MutationWriter::new(&mut self.inner, &mut self.vdom_state);
        self.vdom.rebuild(&mut writer);
    }
}

// Implement Document and required traits for DioxusDocument
impl Document for DioxusDocument {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn handle_ui_event(&mut self, event: UiEvent) {
        set_event_converter(Box::new(NativeConverter {}));
        let handler = DioxusEventHandler {
            vdom: &mut self.vdom,
            vdom_state: &mut self.vdom_state,
        };
        let mut driver = EventDriver::new(self.inner.mutate(), handler);
        driver.handle_ui_event(event);
    }

    fn poll(&mut self, cx: Option<TaskContext>) -> bool {
        {
            let fut = self.vdom.wait_for_work();
            pin_mut!(fut);

            static NOOP_WAKER: LazyLock<Waker> = LazyLock::new(noop_waker);
            let mut cx = cx.unwrap_or_else(|| TaskContext::from_waker(&NOOP_WAKER));
            match fut.poll_unpin(&mut cx) {
                std::task::Poll::Ready(_) => {}
                std::task::Poll::Pending => return false,
            }
        }

        let mut writer = MutationWriter::new(&mut self.inner, &mut self.vdom_state);
        self.vdom.render_immediate(&mut writer);

        true
    }
}

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

pub struct DioxusEventHandler<'v> {
    vdom: &'v mut VirtualDom,
    #[allow(dead_code, reason = "WIP")]
    vdom_state: &'v mut DioxusState,
}

impl EventHandler for DioxusEventHandler<'_> {
    fn handle_event(
        &mut self,
        chain: &[usize],
        event: &mut DomEvent,
        mutr: &mut blitz_dom::DocumentMutator<'_>,
        event_state: &mut EventState,
    ) {
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
                values: Vec::new(),
            })),

            // TODO: Implement IME handling
            DomEventData::Ime(_) => None,
        };

        let Some(event_data) = event_data else {
            return;
        };

        for &node_id in chain {
            // Get dioxus vdom id for node
            let dioxus_id = mutr.doc.get_node(node_id).and_then(get_dioxus_id);
            let Some(id) = dioxus_id else {
                continue;
            };

            // Handle event in vdom
            let dx_event = Event::new(event_data.clone(), event.bubbles);
            self.vdom
                .runtime()
                .handle_event(event.name(), dx_event.clone(), id);

            // Update event state
            if !dx_event.default_action_enabled() {
                event_state.prevent_default();
            }
            if !dx_event.propagates() {
                event_state.stop_propagation();
                break;
            }
        }
    }
}
