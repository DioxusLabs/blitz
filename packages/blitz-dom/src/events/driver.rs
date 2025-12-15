use crate::{BaseDocument, DocumentMutator};
use blitz_traits::events::{BlitzMouseButtonEvent, DomEvent, DomEventData, EventState, UiEvent};
use std::{collections::VecDeque};

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
                let changed = self.doc_mut().set_hover_to(dom_x, dom_y);

                let old_chain = hover_node_id.map(|id| self.doc().node_chain(id));
                let new_chain = self.doc().hover_node_id.map(|id| self.doc().node_chain(id));

                // Find the difference in the node chain of the last hovered objected and the newest
                let mut chain_diff = 0;
                if let Some(ref old_chain) = old_chain {
                    if let Some(ref new_chain) = new_chain {
                        let old_len = old_chain.len();
                        let new_len = new_chain.len();

                        for i in 0..(old_len.min(new_len)) {
                            if old_chain[old_len - i - 1] != new_chain[new_len - i - 1] {
                                chain_diff = i;
                            }
                        }
                    }
                }

                if changed {
                    if let Some(target) = hover_node_id {
                        self.handle_dom_event(DomEvent::new(target, DomEventData::MouseOut(event.clone())));

                        // Send an mouseleave event to all old elements on the chain
                        if let Some(ref old_chain) = old_chain {
                            for i in chain_diff..old_chain.len() {
                                self.handle_dom_event(DomEvent::new(old_chain[old_chain.len() - i - 1], DomEventData::MouseLeave(event.clone())));
                            }
                        }                         
                    }
                }
                hover_node_id = self.doc().hover_node_id;

                if changed {
                    if let Some(target) = hover_node_id {
                        self.handle_dom_event(DomEvent::new(target, DomEventData::MouseOver(event.clone())));

                        // Send an mouseenter event to all new elements on the chain
                        if let Some(ref new_chain) = new_chain {
                            for i in chain_diff..new_chain.len() {
                                self.handle_dom_event(DomEvent::new(new_chain[new_chain.len() - i - 1], DomEventData::MouseEnter(event.clone())));
                            }
                        }
                    }
                }
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
            UiEvent::Wheel(_) => hover_node_id,
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
            UiEvent::Wheel(data) => DomEventData::Wheel(data),
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
