//! Integration with blitz-dom's event driver

use std::cell::RefCell;

use blitz_dom::{
    BaseDocument, Document, EventHandler, EventListenerId, EventListenerOptions, NodeId,
};
use blitz_traits::events::{DomEvent, DomEventKind, EventState};

use crate::runtime::ScriptRuntime;

/// The id of the listener which [`register_script_listeners`] registers
const SCRIPT_LISTENER_ID: EventListenerId = EventListenerId(0);

/// Register a capturing listener for every kind of event on the root element of the document
/// (a no-op if they are already registered).
///
/// JavaScript event listeners are stored (and dispatched to) by the script runtime rather than
/// by blitz-dom's listener registry, so the runtime needs to be invoked exactly once for each
/// event, before any other node's listeners.
pub(crate) fn register_script_listeners(doc: &mut BaseDocument) {
    let Some(root_id) = doc.try_root_element().map(|el| el.id) else {
        return;
    };
    let options = EventListenerOptions {
        capture: true,
        ..Default::default()
    };
    for kind in DomEventKind::ALL {
        doc.add_event_listener(root_id, kind, SCRIPT_LISTENER_ID, options);
    }
}

/// An [`EventHandler`] which dispatches DOM events to JavaScript event listeners
/// before Blitz's default actions run.
pub(crate) struct ScriptEventHandler<'rt> {
    pub runtime: RefCell<&'rt mut ScriptRuntime>,
}

impl EventHandler for ScriptEventHandler<'_> {
    fn handle_event_listener(
        &self,
        _listener: EventListenerId,
        event: &mut DomEvent,
        doc: &mut dyn Document,
        event_state: &mut EventState,
    ) {
        let chain: Vec<NodeId> = if event.bubbles {
            doc.inner().node_chain(event.target)
        } else {
            vec![event.target]
        };
        self.runtime
            .borrow_mut()
            .dispatch_dom_event(&chain, event, event_state);
    }
}
