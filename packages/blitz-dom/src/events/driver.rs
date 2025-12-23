use crate::Document;
use blitz_traits::events::{BlitzMouseButtonEvent, DomEvent, DomEventData, EventState, UiEvent};
use std::collections::VecDeque;

pub trait EventHandler {
    fn handle_event(
        &mut self,
        chain: &[usize],
        event: &mut DomEvent,
        doc: &mut dyn Document,
        event_state: &mut EventState,
    );
}

pub struct NoopEventHandler;
impl EventHandler for NoopEventHandler {
    fn handle_event(
        &mut self,
        _chain: &[usize],
        _event: &mut DomEvent,
        _doc: &mut dyn Document,
        _event_state: &mut EventState,
    ) {
        // Do nothing
    }
}

pub struct EventDriver<'doc, Handler: EventHandler> {
    doc: &'doc mut dyn Document,
    handler: Handler,
}

impl<'doc, Handler: EventHandler> EventDriver<'doc, Handler> {
    pub fn new(doc: &'doc mut dyn Document, handler: Handler) -> Self {
        EventDriver { doc, handler }
    }

    pub fn handle_ui_event(&mut self, event: UiEvent) {
        let doc = self.doc.inner();
        let viewport_scroll = doc.viewport_scroll();
        let zoom = doc.viewport.zoom();

        let mut hover_node_id = doc.hover_node_id;
        let focussed_node_id = doc.focus_node_id;
        drop(doc);

        // Update document input state (hover, focus, active, etc)
        match &event {
            UiEvent::MouseMove(event) => {
                let mut doc = self.doc.inner_mut();
                let dom_x = event.x + viewport_scroll.x as f32 / zoom;
                let dom_y = event.y + viewport_scroll.y as f32 / zoom;
                let changed = doc.set_hover_to(dom_x, dom_y);

                let prev_hover_node_id = hover_node_id;
                hover_node_id = doc.hover_node_id;

                drop(doc);

                if changed {
                    let doc = self.doc.inner();
                    let mut old_chain = prev_hover_node_id
                        .map(|id| doc.node_chain(id))
                        .unwrap_or_default();
                    let mut new_chain = hover_node_id
                        .map(|id| doc.node_chain(id))
                        .unwrap_or_default();
                    old_chain.reverse();
                    new_chain.reverse();

                    // Find the difference in the node chain of the last hovered objected and the newest
                    let old_len = old_chain.len();
                    let new_len = new_chain.len();

                    let first_difference_index = old_chain
                        .iter()
                        .zip(&new_chain)
                        .position(|(old, new)| old != new)
                        .unwrap_or_else(|| old_len.min(new_len));

                    drop(doc);

                    if let Some(target) = prev_hover_node_id {
                        self.handle_dom_event(DomEvent::new(
                            target,
                            DomEventData::MouseOut(event.clone()),
                        ));

                        // Send an mouseleave event to all old elements on the chain
                        for node_id in old_chain
                            .get(first_difference_index..)
                            .unwrap_or(&[])
                            .iter()
                        {
                            self.handle_dom_event(DomEvent::new(
                                *node_id,
                                DomEventData::MouseLeave(event.clone()),
                            ));
                        }
                    }

                    if let Some(target) = hover_node_id {
                        self.handle_dom_event(DomEvent::new(
                            target,
                            DomEventData::MouseOver(event.clone()),
                        ));

                        // Send an mouseenter event to all new elements on the chain
                        for node_id in new_chain
                            .get(first_difference_index..)
                            .unwrap_or(&[])
                            .iter()
                        {
                            self.handle_dom_event(DomEvent::new(
                                *node_id,
                                DomEventData::MouseEnter(event.clone()),
                            ));
                        }
                    }
                }
            }
            UiEvent::MouseDown(_) => {
                let mut doc = self.doc.inner_mut();
                doc.active_node();
                doc.set_mousedown_node_id(hover_node_id);
            }
            UiEvent::MouseUp(_) => {
                let mut doc = self.doc.inner_mut();
                doc.unactive_node();
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

        let target = target.unwrap_or_else(|| self.doc.inner().root_element().id);
        let dom_event = DomEvent::new(target, data);

        self.handle_dom_event(dom_event);
    }

    pub fn handle_dom_event(&mut self, event: DomEvent) {
        let mut queue = VecDeque::with_capacity(4);
        queue.push_back(event);

        while let Some(mut event) = queue.pop_front() {
            let chain = if event.bubbles {
                let doc = self.doc.inner();
                doc.node_chain(event.target)
            } else {
                vec![event.target]
            };

            let mut event_state = EventState::default();
            self.handler
                .handle_event(&chain, &mut event, self.doc, &mut event_state);

            if !event_state.is_cancelled() {
                let mut doc = self.doc.inner_mut();
                doc.handle_dom_event(&mut event, |new_evt| queue.push_back(new_evt));
            }
        }
    }
}
