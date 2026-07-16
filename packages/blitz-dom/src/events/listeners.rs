//! Browser-like event listener registry (`addEventListener` / `removeEventListener`)
use blitz_traits::events::DomEventKind;
use std::collections::HashMap;

/// An opaque identifier for an event listener.
///
/// Blitz does not store listener callbacks itself. Instead, listeners are registered with an
/// embedder-allocated id which plays the same role that callback identity plays in the browser's
/// `addEventListener` API: registering the same id for the same event kind (and capture flag) on
/// the same node twice is a no-op, and the id is what is passed to
/// [`remove_event_listener`](crate::BaseDocument::remove_event_listener).
///
/// When an event is dispatched, the [`EventHandler`](crate::EventHandler) is invoked with the id
/// of each matching listener and is responsible for mapping that id back to actual behaviour
/// (a Rust closure, a Dioxus vdom handler, a script function, etc). See
/// [`CallbackEventHandler`](crate::CallbackEventHandler) for a ready-made closure-based mapping.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EventListenerId(pub u64);

/// Options for [`add_event_listener`](crate::BaseDocument::add_event_listener).
///
/// Mirrors the `options` parameter of the DOM `addEventListener` API.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct EventListenerOptions {
    /// Invoke the listener during the capture phase (root -> target) rather than
    /// during the target/bubble phases.
    pub capture: bool,
    /// Automatically remove the listener after it has been invoked once.
    pub once: bool,
    /// The listener will not be able to cancel the event with `prevent_default`.
    pub passive: bool,
}

/// A single registered event listener
#[derive(Copy, Clone, Debug)]
pub struct EventListener {
    pub kind: DomEventKind,
    pub id: EventListenerId,
    pub options: EventListenerOptions,
}

/// The set of event listeners registered on the nodes of a document
pub(crate) struct EventListenerRegistry {
    /// Listeners registered per node (in registration order)
    listeners: HashMap<usize, Vec<EventListener>>,
    /// Count of registered listeners per event kind (indexed by `DomEventKind` discriminant).
    /// Used to skip dispatch entirely for events which nobody is listening to.
    counts: [u32; 64],
}

impl Default for EventListenerRegistry {
    fn default() -> Self {
        Self {
            listeners: HashMap::new(),
            counts: [0; 64],
        }
    }
}

impl EventListenerRegistry {
    /// Register a listener on a node. Returns `false` (and does nothing) if a listener with
    /// the same event kind, id and capture flag is already registered on the node.
    pub fn add(
        &mut self,
        node_id: usize,
        kind: DomEventKind,
        id: EventListenerId,
        options: EventListenerOptions,
    ) -> bool {
        let list = self.listeners.entry(node_id).or_default();
        // Per addEventListener semantics a listener is identified by (kind, id, capture),
        // and re-registering an existing listener is a no-op.
        if list
            .iter()
            .any(|l| l.kind == kind && l.id == id && l.options.capture == options.capture)
        {
            return false;
        }
        list.push(EventListener { kind, id, options });
        self.counts[kind.discriminant() as usize] += 1;
        true
    }

    /// Remove a listener from a node. Returns `false` if no matching listener was registered.
    pub fn remove(
        &mut self,
        node_id: usize,
        kind: DomEventKind,
        id: EventListenerId,
        capture: bool,
    ) -> bool {
        let Some(list) = self.listeners.get_mut(&node_id) else {
            return false;
        };
        let Some(idx) = list
            .iter()
            .position(|l| l.kind == kind && l.id == id && l.options.capture == capture)
        else {
            return false;
        };
        list.remove(idx);
        if list.is_empty() {
            self.listeners.remove(&node_id);
        }
        self.counts[kind.discriminant() as usize] -= 1;
        true
    }

    /// Remove all listeners registered on a node (used when the node is dropped)
    pub fn remove_all_for_node(&mut self, node_id: usize) {
        if let Some(list) = self.listeners.remove(&node_id) {
            for listener in list {
                self.counts[listener.kind.discriminant() as usize] -= 1;
            }
        }
    }

    /// Whether any node has a listener for the given event kind
    pub fn has_listeners_for_kind(&self, kind: DomEventKind) -> bool {
        self.counts[kind.discriminant() as usize] > 0
    }

    /// Whether the given listener is (still) registered on the given node
    pub fn contains(
        &self,
        node_id: usize,
        kind: DomEventKind,
        id: EventListenerId,
        capture: bool,
    ) -> bool {
        self.listeners.get(&node_id).is_some_and(|list| {
            list.iter()
                .any(|l| l.kind == kind && l.id == id && l.options.capture == capture)
        })
    }

    /// Collect the listeners for the given event kind on the given node (in registration order).
    ///
    /// Returns an owned snapshot so that dispatch is not affected by listeners added
    /// during dispatch (matching DOM dispatch semantics).
    pub fn listeners_for(&self, node_id: usize, kind: DomEventKind) -> Vec<EventListener> {
        self.listeners
            .get(&node_id)
            .map(|list| {
                list.iter()
                    .filter(|l| l.kind == kind)
                    .copied()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }
}
