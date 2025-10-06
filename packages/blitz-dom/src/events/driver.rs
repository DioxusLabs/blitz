use crate::{BaseDocument, DocumentMutator};
use blitz_traits::events::{BlitzMouseButtonEvent, DomEvent, DomEventData, EventState, UiEvent};
use std::collections::VecDeque;

pub trait EventHandler {
    fn handle_event(
        &mut self,
        chain: &[usize],
        event: &mut DomEvent,
        mutr: &mut DocumentMutator<'_>,
        event_state: &mut EventState,
    );
}

pub struct NoopEventHandler;
impl EventHandler for NoopEventHandler {
    fn handle_event(
        &mut self,
        _chain: &[usize],
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

    pub fn handle_ui_event(&mut self, event: UiEvent) {
        let viewport_scroll = self.doc().viewport_scroll();
        let zoom = self.doc().viewport.zoom();

        let mut hover_node_id = self.doc().hover_node_id;
        let focussed_node_id = self.doc().focus_node_id;

        // Update document input state (hover, focus, active, etc)
        match &event {
            UiEvent::MouseMove(event) => {
                let dom_x = event.x + viewport_scroll.x as f32 / zoom;
                let dom_y = event.y + viewport_scroll.y as f32 / zoom;
                self.doc_mut().set_hover_to(dom_x, dom_y);
                hover_node_id = self.doc().hover_node_id;
            }
            UiEvent::MouseDown(_) => {
                self.doc_mut().active_node();
                self.doc_mut().set_mousedown_node_id(hover_node_id);
            }
            UiEvent::MouseUp(_) => {
                self.doc_mut().unactive_node();
            }
            _ => {}
        };

        let target = match event {
            UiEvent::MouseMove(_) => hover_node_id,
            UiEvent::MouseUp(_) => hover_node_id,
            UiEvent::MouseDown(_) => hover_node_id,
            UiEvent::KeyUp(_) => focussed_node_id,
            UiEvent::KeyDown(_) => focussed_node_id,
            UiEvent::Ime(_) => focussed_node_id,
        };

        let data = match event {
            UiEvent::MouseMove(data) => DomEventData::MouseMove(BlitzMouseButtonEvent {
                x: data.x + viewport_scroll.x as f32 / zoom,
                y: data.y + viewport_scroll.y as f32 / zoom,
                ..data
            }),
            UiEvent::MouseUp(data) => DomEventData::MouseUp(BlitzMouseButtonEvent {
                x: data.x + viewport_scroll.x as f32 / zoom,
                y: data.y + viewport_scroll.y as f32 / zoom,
                ..data
            }),
            UiEvent::MouseDown(data) => DomEventData::MouseDown(BlitzMouseButtonEvent {
                x: data.x + viewport_scroll.x as f32 / zoom,
                y: data.y + viewport_scroll.y as f32 / zoom,
                ..data
            }),
            UiEvent::KeyUp(data) => DomEventData::KeyUp(data),
            UiEvent::KeyDown(data) => DomEventData::KeyDown(data),
            UiEvent::Ime(data) => DomEventData::Ime(data),
        };

        let target = target.unwrap_or_else(|| self.doc().root_element().id);
        let dom_event = DomEvent::new(target, data);

        self.handle_dom_event(dom_event);
    }

    pub fn handle_dom_event(&mut self, event: DomEvent) {
        let mut queue = VecDeque::with_capacity(4);
        queue.push_back(event);

        while let Some(mut event) = queue.pop_front() {
            let chain = if event.bubbles {
                self.doc().node_chain(event.target)
            } else {
                vec![event.target]
            };

            let mut event_state = EventState::default();
            self.handler
                .handle_event(&chain, &mut event, &mut self.mutr, &mut event_state);

            if !event_state.is_cancelled() {
                self.doc_mut()
                    .handle_dom_event(&mut event, |new_evt| queue.push_back(new_evt));
            }
        }
    }
}
