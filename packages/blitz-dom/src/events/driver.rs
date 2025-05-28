use crate::{BaseDocument, DocumentMutator};
use blitz_traits::{DomEvent, EventState};
use std::collections::VecDeque;

pub trait EventHandler {
    fn handle_event(
        &mut self,
        node_id: usize,
        event: &mut DomEvent,
        mutr: &mut DocumentMutator<'_>,
        event_state: &mut EventState,
    );
}

pub struct NoopEventHandler;
impl EventHandler for NoopEventHandler {
    fn handle_event(
        &mut self,
        _node_id: usize,
        _event: &mut DomEvent,
        _mutr: &mut DocumentMutator<'_>,
        _event_state: &mut EventState,
    ) {
        // Do nothing
    }
}

pub struct EventDriver<'doc, Handler: EventHandler> {
    mutr: DocumentMutator<'doc>,
    handler: Handler,
}

impl<'doc, Handler: EventHandler> EventDriver<'doc, Handler> {
    fn doc_mut(&mut self) -> &mut BaseDocument {
        self.mutr.doc
    }

    fn doc(&self) -> &BaseDocument {
        &*self.mutr.doc
    }

    pub fn new(mutr: DocumentMutator<'doc>, handler: Handler) -> Self {
        EventDriver { mutr, handler }
    }

    pub fn handle_event(&mut self, event: DomEvent) {
        let mut queue = VecDeque::with_capacity(4);
        queue.push_back(event);

        while let Some(mut event) = queue.pop_front() {
            let chain = self.doc().node_chain(event.target);

            let mut event_state = EventState::default();
            for node_id in chain.clone().into_iter() {
                self.handler
                    .handle_event(node_id, &mut event, &mut self.mutr, &mut event_state);
                if !event.bubbles | event_state.propagation_is_stopped() {
                    break;
                }
            }

            if !event_state.is_cancelled() {
                self.doc_mut()
                    .handle_event(&mut event, |new_evt| queue.push_back(new_evt));
            }
        }
    }
}
