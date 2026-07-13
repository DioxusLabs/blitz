//! A closure-based [`EventHandler`] for embedders which use blitz-dom directly
use crate::events::driver::EventHandler;
use crate::events::listeners::{EventListenerId, EventListenerOptions};
use crate::{BaseDocument, Document};
use blitz_traits::events::{DomEvent, DomEventKind, EventState};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The type of callback closures registered with a [`CallbackEventHandler`]
pub type EventListenerCallback = dyn FnMut(&mut DomEvent, &mut dyn Document, &mut EventState);

/// A convenience [`EventHandler`] which maps [`EventListenerId`]s to Rust closures, giving
/// applications that use blitz-dom directly (without a framework such as Dioxus) browser-like
/// `addEventListener` ergonomics:
///
/// ```rust
/// use blitz_dom::{BaseDocument, CallbackEventHandler, DocumentConfig, EventDriver, EventListenerOptions};
/// use blitz_traits::events::DomEventKind;
///
/// let mut doc = BaseDocument::new(DocumentConfig::default());
/// let node_id = doc.root_node().id;
///
/// let handler = CallbackEventHandler::new();
/// handler.add_event_listener(
///     &mut doc,
///     node_id,
///     DomEventKind::Click,
///     EventListenerOptions::default(),
///     |event, _doc, _state| println!("clicked {}", event.target),
/// );
///
/// // `CallbackEventHandler` is cheaply cloneable (clones share the same callbacks),
/// // so a clone can be handed to each `EventDriver`.
/// let driver = EventDriver::new(&mut doc, handler.clone());
/// ```
///
/// Note: removing a listener with [`BaseDocument::remove_event_listener`] (or via the `once`
/// listener option) does not drop the closure held by this handler. Use
/// [`CallbackEventHandler::remove_event_listener`] or
/// [`CallbackEventHandler::unregister_callback`] to also drop the closure.
#[derive(Clone, Default)]
pub struct CallbackEventHandler {
    inner: Rc<RefCell<CallbackRegistry>>,
}

#[derive(Default)]
struct CallbackRegistry {
    next_id: u64,
    callbacks: HashMap<EventListenerId, Rc<RefCell<EventListenerCallback>>>,
}

impl CallbackEventHandler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a callback closure, returning the [`EventListenerId`] allocated for it.
    /// The id can then be attached to nodes with [`BaseDocument::add_event_listener`].
    pub fn register_callback(
        &self,
        callback: impl FnMut(&mut DomEvent, &mut dyn Document, &mut EventState) + 'static,
    ) -> EventListenerId {
        let mut registry = self.inner.borrow_mut();
        registry.next_id += 1;
        let id = EventListenerId(registry.next_id);
        registry
            .callbacks
            .insert(id, Rc::new(RefCell::new(callback)));
        id
    }

    /// Deregister a callback closure, dropping it.
    ///
    /// Note: this does *not* remove listeners registered on document nodes with this id.
    pub fn unregister_callback(&self, id: EventListenerId) {
        self.inner.borrow_mut().callbacks.remove(&id);
    }

    /// Register a closure as an event listener on a node of the document.
    ///
    /// This is the closure-based equivalent of the DOM `addEventListener` API. The returned
    /// [`EventListenerId`] can be used to remove the listener again with
    /// [`CallbackEventHandler::remove_event_listener`].
    pub fn add_event_listener(
        &self,
        doc: &mut BaseDocument,
        node_id: usize,
        kind: DomEventKind,
        options: EventListenerOptions,
        callback: impl FnMut(&mut DomEvent, &mut dyn Document, &mut EventState) + 'static,
    ) -> EventListenerId {
        let id = self.register_callback(callback);
        doc.add_event_listener(node_id, kind, id, options);
        id
    }

    /// Remove an event listener previously added with
    /// [`CallbackEventHandler::add_event_listener`], removing the listener from the node
    /// and dropping the callback closure.
    pub fn remove_event_listener(
        &self,
        doc: &mut BaseDocument,
        node_id: usize,
        kind: DomEventKind,
        id: EventListenerId,
        capture: bool,
    ) -> bool {
        let removed = doc.remove_event_listener(node_id, kind, id, capture);
        self.unregister_callback(id);
        removed
    }
}

impl EventHandler for CallbackEventHandler {
    fn handle_event_listener(
        &mut self,
        listener: EventListenerId,
        event: &mut DomEvent,
        doc: &mut dyn Document,
        event_state: &mut EventState,
    ) {
        // Clone the callback out of the registry so that the registry is not borrowed while
        // the callback runs (the callback may register/deregister callbacks itself)
        let callback = self.inner.borrow().callbacks.get(&listener).cloned();
        if let Some(callback) = callback {
            (callback.borrow_mut())(event, doc, event_state);
        }
    }
}
