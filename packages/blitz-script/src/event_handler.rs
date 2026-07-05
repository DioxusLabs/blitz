//! Integration with blitz-dom's event driver

use blitz_dom::{Document, EventHandler};
use blitz_traits::events::{DomEvent, EventState};

use crate::runtime::ScriptRuntime;

/// An [`EventHandler`] which dispatches DOM events to JavaScript event listeners
/// before Blitz's default actions run.
pub(crate) struct ScriptEventHandler<'rt> {
    pub runtime: &'rt mut ScriptRuntime,
}

impl EventHandler for ScriptEventHandler<'_> {
    fn handle_event(
        &mut self,
        chain: &[usize],
        event: &mut DomEvent,
        _doc: &mut dyn Document,
        event_state: &mut EventState,
    ) {
        self.runtime.dispatch_dom_event(chain, event, event_state);
    }
}
