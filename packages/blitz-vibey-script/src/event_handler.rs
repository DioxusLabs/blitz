//! Integration with blitz-dom's event driver

use blitz_dom::{EventContext, EventHandler, NodeId};
use blitz_traits::events::{DomEvent, EventState};
use std::cell::RefCell;

use crate::runtime::ScriptRuntime;

/// An [`EventHandler`] which dispatches DOM events to JavaScript event listeners
/// before Blitz's default actions run.
pub(crate) struct ScriptEventHandler<'rt> {
    pub runtime: RefCell<&'rt mut ScriptRuntime>,
}

impl EventHandler for ScriptEventHandler<'_> {
    fn handle_event(
        &self,
        chain: &[NodeId],
        event: &mut DomEvent,
        _ctx: &mut EventContext<'_>,
        event_state: &mut EventState,
    ) {
        self.runtime
            .borrow_mut()
            .dispatch_dom_event(chain, event, event_state);
    }
}
