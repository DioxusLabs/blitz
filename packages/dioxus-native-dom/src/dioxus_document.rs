//! Integration between Dioxus and Blitz
use crate::NodeId;
use crate::events::{
    BlitzKeyboardData, NativeConverter, NativeFocusData, NativeFormData, NativePointerData,
    NativeScrollData, NativeTouchData, NativeWheelData, NodeHandle,
};
use crate::mutation_writer::{DioxusState, MutationWriter};
use crate::qual_name;
use blitz_dom::{
    Attribute, BaseDocument, DEFAULT_CSS, DocGuard, DocGuardMut, Document, DocumentConfig,
    EventContext, EventDriver, EventHandler, EventListenerId, Node,
};
use blitz_traits::events::{DomEvent, DomEventData, EventState, UiEvent};
use dioxus_core::{ElementId, Event, Runtime, VirtualDom};
use dioxus_html::{PlatformEventData, set_event_converter};
use futures_util::task::noop_waker;
use std::cell::RefCell;
use std::future::Future;
use std::mem;
use std::pin::pin;
use std::sync::LazyLock;
use std::task::{Context as TaskContext, Waker};
use std::{any::Any, rc::Rc};

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

/// Integrates [`BaseDocument`] from  [`blitz-dom`](blitz_dom)  with [`VirtualDom`] from [`dioxus-core`](dioxus_core)
///
/// ### Example
///
/// ```rust
/// use blitz_traits::shell::{Viewport, ColorScheme};
/// use dioxus_native_dom::{DioxusDocument, DocumentConfig};
/// use dioxus::prelude::*;
///
/// // Example Dioxus app
/// fn app() -> Element {
///     rsx! {
///         div { "Hello, world!" }
///     }
/// }
///
/// fn main() {
///    let vdom = VirtualDom::new(app);
///    let mut doc = DioxusDocument::new(vdom, DocumentConfig {
///         viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
///         ..Default::default()
///    });
///    doc.initial_build();
/// }
/// ```
///
/// You can just push events into the [`DioxusDocument`] with [`doc.handle_ui_event(..)`](Self::handle_ui_event)
/// and then flush the changes with [`doc.poll(..)`](Self::poll)
pub struct DioxusDocument {
    pub inner: Rc<RefCell<BaseDocument>>,
    pub vdom: VirtualDom,
    pub vdom_state: DioxusState,

    #[allow(unused)]
    pub(crate) html_element_id: NodeId,
    #[allow(unused)]
    pub(crate) head_element_id: NodeId,
    #[allow(unused)]
    pub(crate) body_element_id: NodeId,
    #[allow(unused)]
    pub(crate) main_element_id: NodeId,
}

impl DioxusDocument {
    /// Create a new [`DioxusDocument`] from a [`VirtualDom`].
    pub fn new(vdom: VirtualDom, mut config: DocumentConfig) -> Self {
        // Only really needs to happen once
        set_event_converter(Box::new(NativeConverter {}));

        config.base_url = Some(
            config
                .base_url
                .unwrap_or_else(|| String::from("dioxus://index.html")),
        );
        let mut doc = BaseDocument::new(config);

        // Include default stylesheet
        doc.add_user_agent_stylesheet(DEFAULT_CSS);

        // Create some minimal HTML to render the app into.
        // HTML is equivalent to:
        //
        // <html>
        // <head></head>
        // <body>
        //    <div id="main"></div>
        // </body>
        // </html>
        //
        // TODO: Support arbitrary "index.html" templates

        // Create the html element
        let mut mutr = doc.mutate();
        let html_element_id = mutr.create_element(qual_name("html", None), vec![]);
        mutr.append_children(mutr.doc.root_node().id, &[html_element_id]);

        // Create the body element
        let head_element_id = mutr.create_element(qual_name("head", None), vec![]);
        mutr.append_children(html_element_id, &[head_element_id]);

        // Create the body element
        let body_element_id = mutr.create_element(qual_name("body", None), vec![]);
        mutr.append_children(html_element_id, &[body_element_id]);

        // Create another virtual element to hold the root <div id="main"></div> under the html element
        let main_attrs = vec![
            blitz_dom::Attribute {
                name: qual_name("id", None),
                value: "main".to_string(),
            },
            // The root element maps to ElementId(0) in the vdom (mirrors dioxus-web,
            // which sets data-dioxus-id="0" on its root element)
            blitz_dom::Attribute {
                name: qual_name("data-dioxus-id", None),
                value: "0".to_string(),
            },
        ];
        let main_element_id = mutr.create_element(qual_name("main", None), main_attrs);
        mutr.append_children(body_element_id, &[main_element_id]);

        drop(mutr);

        let vdom_state = DioxusState::create(main_element_id);
        Self {
            vdom,
            vdom_state,
            inner: Rc::new(RefCell::new(doc)),
            html_element_id,
            head_element_id,
            body_element_id,
            main_element_id,
        }
    }

    /// Run an initial build of the Dioxus vdom
    pub fn initial_build(&mut self) {
        let mut inner = self.inner.borrow_mut();
        let mut writer = MutationWriter::new(&mut inner, &mut self.vdom_state);
        self.vdom.rebuild(&mut writer);
        drop(writer);
        drop(inner);
        self.flush_queued_mounted_events();
    }

    /// Used to respond to a `CreateHeadElement` event generated by Dioxus. These
    /// events allow Dioxus to create elements in the `<head>` of the document.
    #[doc(hidden)]
    pub fn create_head_element(
        &mut self,
        name: &str,
        attributes: &[(String, String)],
        contents: &Option<String>,
    ) {
        let mut inner = self.inner.borrow_mut();
        let mut mutr = inner.mutate();

        let attributes = attributes
            .iter()
            .map(|(name, value)| Attribute {
                name: qual_name(name, None),
                value: value.clone(),
            })
            .collect();

        let new_elem_id = mutr.create_element(qual_name(name, None), attributes);
        mutr.append_children(self.head_element_id, &[new_elem_id]);
        if let Some(contents) = contents {
            let text_node_id = mutr.create_text_node(contents);
            mutr.append_children(new_elem_id, &[text_node_id]);
        }
    }

    pub(crate) fn flush_queued_mounted_events(&mut self) {
        let mut queued_mounted_events = mem::take(&mut self.vdom_state.queued_mounted_events);
        for element_id in queued_mounted_events.drain(..) {
            let node_id = self.vdom_state.element_to_node_id(element_id);

            if self.inner.borrow().get_node(node_id).is_some() {
                let event = Event::new(
                    Rc::new(PlatformEventData::new(Box::new(NodeHandle {
                        doc: Rc::clone(&self.inner),
                        runtime: self.vdom.runtime(),
                        node_id,
                    }))) as Rc<dyn Any>,
                    false,
                );
                self.vdom
                    .runtime()
                    .handle_event("mounted", event, element_id);
            }
        }

        self.vdom_state.queued_mounted_events = queued_mounted_events;
    }
}

// Implement DocumentLike and required traits for DioxusDocument
impl Document for DioxusDocument {
    fn id(&self) -> usize {
        self.inner.borrow().id()
    }

    fn inner(&self) -> DocGuard<'_> {
        DocGuard::RefCell(self.inner.borrow())
    }

    fn inner_mut(&mut self) -> DocGuardMut<'_> {
        DocGuardMut::RefCell(self.inner.borrow_mut())
    }

    fn poll(&mut self, cx: Option<TaskContext>) -> bool {
        {
            let fut = self.vdom.wait_for_work();
            let mut pinned_fut = pin!(fut);

            static NOOP_WAKER: LazyLock<Waker> = LazyLock::new(noop_waker);
            let mut cx = cx.unwrap_or_else(|| TaskContext::from_waker(&NOOP_WAKER));
            match pinned_fut.as_mut().poll(&mut cx) {
                std::task::Poll::Ready(_) => {}
                std::task::Poll::Pending => return false,
            }
        }

        let mut inner = self.inner.borrow_mut();
        let mut writer = MutationWriter::new(&mut inner, &mut self.vdom_state);
        self.vdom.render_immediate(&mut writer);
        drop(writer);
        drop(inner);
        self.flush_queued_mounted_events();

        // Dispatch any events queued by the vdom update (e.g. focus events generated
        // by a `mounted` handler calling `set_focus`)
        if self.inner.borrow().has_pending_events() {
            self.flush_pending_dom_events();
        }

        true
    }

    fn handle_ui_event(&mut self, event: UiEvent) {
        let handler = DioxusEventHandler {
            runtime: self.vdom.runtime(),
        };
        let mut driver = EventDriver::new(&mut self.inner, handler);
        driver.handle_ui_event(event);
    }
}

impl DioxusDocument {
    /// Dispatch any events queued on the document (see [`BaseDocument::queue_event`])
    /// to the Dioxus vdom
    fn flush_pending_dom_events(&mut self) {
        let handler = DioxusEventHandler {
            runtime: self.vdom.runtime(),
        };
        let mut driver = EventDriver::new(&mut self.inner, handler);
        driver.flush_pending_events();
    }
}

/// An [`EventHandler`] which routes events to Dioxus vdom event handlers.
///
/// Holds an `Rc<Runtime>` (rather than a reference to the `VirtualDom`) so that it can be
/// re-entered if a vdom event handler synchronously triggers the dispatch of further events
/// (mirroring dioxus-web, whose delegated event handler closure captures an `Rc<Runtime>`).
pub struct DioxusEventHandler {
    runtime: Rc<Runtime>,
}

impl DioxusEventHandler {
    pub fn new(runtime: Rc<Runtime>) -> Self {
        Self { runtime }
    }
}

impl EventHandler for DioxusEventHandler {
    fn handle_event_listener(
        &self,
        _listener: EventListenerId,
        event: &mut DomEvent,
        ctx: &mut EventContext<'_>,
        event_state: &mut EventState,
    ) {
        let event_data = match &event.data {
            DomEventData::PointerMove(mevent)
            | DomEventData::PointerDown(mevent)
            | DomEventData::PointerUp(mevent)
            | DomEventData::PointerCancel(mevent)
            | DomEventData::PointerLeave(mevent)
            | DomEventData::PointerEnter(mevent)
            | DomEventData::PointerOver(mevent)
            | DomEventData::PointerOut(mevent)
            | DomEventData::MouseMove(mevent)
            | DomEventData::MouseDown(mevent)
            | DomEventData::MouseUp(mevent)
            | DomEventData::MouseLeave(mevent)
            | DomEventData::MouseEnter(mevent)
            | DomEventData::MouseOver(mevent)
            | DomEventData::MouseOut(mevent)
            | DomEventData::Click(mevent)
            | DomEventData::ContextMenu(mevent)
            | DomEventData::DoubleClick(mevent) => {
                Some(wrap_event_data(NativePointerData(mevent.clone())))
            }

            DomEventData::TouchStart(tevent)
            | DomEventData::TouchMove(tevent)
            | DomEventData::TouchEnd(tevent)
            | DomEventData::TouchCancel(tevent) => {
                Some(wrap_event_data(NativeTouchData(tevent.clone())))
            }

            DomEventData::Scroll(sevent) => Some(wrap_event_data(NativeScrollData(sevent.clone()))),
            DomEventData::Wheel(wevent) => Some(wrap_event_data(NativeWheelData(wevent.clone()))),

            DomEventData::Focus(_) => Some(wrap_event_data(NativeFocusData)),
            DomEventData::Blur(_) => Some(wrap_event_data(NativeFocusData)),
            DomEventData::FocusIn(_) => Some(wrap_event_data(NativeFocusData)),
            DomEventData::FocusOut(_) => Some(wrap_event_data(NativeFocusData)),

            DomEventData::KeyDown(kevent)
            | DomEventData::KeyUp(kevent)
            | DomEventData::KeyPress(kevent) => {
                Some(wrap_event_data(BlitzKeyboardData(kevent.clone())))
            }

            DomEventData::Input(data) => Some(wrap_event_data(NativeFormData {
                value: data.value.clone(),
                values: vec![],
            })),

            // TODO: Implement IME handling
            DomEventData::Ime(_) => None,

            // AppleStandardKeybinding events are not exposed to script
            DomEventData::AppleStandardKeybinding(_) => None,
        };

        let Some(event_data) = event_data else {
            return;
        };

        // Find the nearest element at-or-above the event target with a `data-dioxus-id`
        // attribute. This mirrors dioxus-web's delegated event handling: the target of a
        // delegated event may be a descendant of the element with the vdom event handler.
        let dioxus_id = {
            let doc = ctx.doc().inner();
            doc.node_chain(event.target)
                .into_iter()
                .find_map(|node_id| doc.get_node(node_id).and_then(get_dioxus_id))
        };
        let Some(id) = dioxus_id else {
            return;
        };

        // Handle event in vdom (dioxus-core does its own bubbling through the vdom)
        let dx_event = Event::new(event_data, event.bubbles);
        self.runtime
            .handle_event(event.name(), dx_event.clone(), id);

        // Update event state
        if !dx_event.default_action_enabled() {
            event_state.prevent_default();
        }
        if !dx_event.propagates() {
            event_state.stop_propagation();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_dom::DocumentConfig;
    use dioxus::prelude::*;
    use dioxus_core::ScopeId;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[test]
    // Regression test for a panic. The keyed `div`s are re-ordered as the (unordered) `HashMap` grows,
    // which previously caused a crash when moving keyed nodes within their parent.
    fn keyed_nodes_do_not_crash() {
        type SharedData = Rc<RefCell<HashMap<usize, usize>>>;
        let data: SharedData = Rc::new(RefCell::new(HashMap::new()));

        fn app(data: SharedData) -> Element {
            let entries: Vec<usize> = data.borrow().keys().copied().collect();
            rsx!(
                for id in entries {
                    div {
                        key: "item_{id}",
                        "{id}"
                    }
                }
            )
        }

        let vdom = VirtualDom::new_with_props(app, Rc::clone(&data));
        let mut doc = DioxusDocument::new(vdom, DocumentConfig::default());
        doc.initial_build();

        // Mirror `examples/crash.rs`: incrementally insert 100 items, flushing
        // the resulting mutations after each insert.
        for i in 0..100 {
            data.borrow_mut().insert(i, i);
            doc.vdom.mark_dirty(ScopeId::APP);
            doc.poll(None);
        }

        // The `<main>` element should end up with exactly one keyed `div` per
        // inserted item, and applying the mutations must not have panicked.
        let inner = doc.inner.borrow();
        let main = inner.get_node(doc.main_element_id).unwrap();
        assert_eq!(main.children.len(), 100);
    }
}
