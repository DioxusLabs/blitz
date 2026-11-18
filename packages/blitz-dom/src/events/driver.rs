use crate::Document;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, DomEvent, DomEventData, EventDispatchSource, EventState,
    Point, PointerCoords, UiEvent,
};
use blitz_traits::node_id::NodeId;

pub trait EventHandler {
    fn handle_event(
        &mut self,
        chain: &[NodeId],
        event: &mut DomEvent,
        doc: &mut dyn Document,
        event_state: &mut EventState,
    );
}

pub struct NoopEventHandler;
impl EventHandler for NoopEventHandler {
    fn handle_event(
        &mut self,
        _chain: &[NodeId],
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

    pub fn handle_pointer_move(&mut self, event: &BlitzPointerEvent) -> Option<NodeId> {
        let mut doc = self.doc.inner_mut();

        let prev_hover_node_id = doc.hover_node_id;
        let changed = doc.set_hover_to(event.page_x(), event.page_y());
        let hover_node_id = doc.hover_node_id;

        drop(doc);

        if !changed {
            return prev_hover_node_id;
        }

        self.dispatch_hover_change(prev_hover_node_id, hover_node_id, event);

        hover_node_id
    }

    /// End hover when the pointer leaves the document, dispatching out/leave
    /// events to the previously hovered chain
    pub fn handle_pointer_leave(&mut self, event: &BlitzPointerEvent) {
        let mut doc = self.doc.inner_mut();
        let prev_hover_node_id = doc.hover_node_id;
        doc.clear_hover();
        drop(doc);

        self.dispatch_hover_change(prev_hover_node_id, None, event);
    }

    fn dispatch_hover_change(
        &mut self,
        prev_hover_node_id: Option<NodeId>,
        hover_node_id: Option<NodeId>,
        event: &BlitzPointerEvent,
    ) {
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

        let is_mouse = event.is_mouse();

        if let Some(target) = prev_hover_node_id {
            self.handle_dom_event(DomEvent::new(
                target,
                DomEventData::PointerOut(event.clone()),
                EventDispatchSource::Input,
            ));
            if is_mouse {
                self.handle_dom_event(DomEvent::new(
                    target,
                    DomEventData::MouseOut(event.clone()),
                    EventDispatchSource::Input,
                ));
            }

            // Send an mouseleave event to all old elements on the chain
            for node_id in old_chain
                .get(first_difference_index..)
                .unwrap_or(&[])
                .iter()
            {
                self.handle_dom_event(DomEvent::new(
                    *node_id,
                    DomEventData::PointerLeave(event.clone()),
                    EventDispatchSource::Input,
                ));
                if is_mouse {
                    self.handle_dom_event(DomEvent::new(
                        *node_id,
                        DomEventData::MouseLeave(event.clone()),
                        EventDispatchSource::Input,
                    ));
                }
            }
        }

        if let Some(target) = hover_node_id {
            self.handle_dom_event(DomEvent::new(
                target,
                DomEventData::PointerOver(event.clone()),
                EventDispatchSource::Input,
            ));

            if is_mouse {
                self.handle_dom_event(DomEvent::new(
                    target,
                    DomEventData::MouseOver(event.clone()),
                    EventDispatchSource::Input,
                ));
            }

            // Send an mouseenter event to all new elements on the chain
            for node_id in new_chain
                .get(first_difference_index..)
                .unwrap_or(&[])
                .iter()
            {
                self.handle_dom_event(DomEvent::new(
                    *node_id,
                    DomEventData::PointerEnter(event.clone()),
                    EventDispatchSource::Input,
                ));

                if is_mouse {
                    self.handle_dom_event(DomEvent::new(
                        *node_id,
                        DomEventData::MouseEnter(event.clone()),
                        EventDispatchSource::Input,
                    ));
                }
            }
        }
    }

    pub fn handle_ui_event(&mut self, event: UiEvent) {
        // Events queued on the document since a driver last ran come before the new event
        self.flush_pending_events();

        let doc = self.doc.inner();

        // A non-hovering pointer (finger) ends hover once it is lifted or cancelled
        let mut hover_end_event = None;
        let mut hover_node_id = doc.hover_node_id;
        let focussed_node_id = doc.focus_node_id;
        let prev_hover = doc.hover_state();
        drop(doc);

        // A document the pointer has left has no say over the cursor
        let is_leave = matches!(event, UiEvent::PointerLeave(_));

        // Update document input state (hover, focus, active, etc)
        match &event {
            UiEvent::PointerMove(event) => {
                hover_node_id = self.handle_pointer_move(event);
            }
            UiEvent::PointerDown(event) => {
                hover_node_id = self.handle_pointer_move(event);
                let mut doc = self.doc.inner_mut();
                doc.active_node();
                doc.set_mousedown_node_id(hover_node_id);
            }
            UiEvent::PointerUp(event) => {
                hover_node_id = self.handle_pointer_move(event);
                let mut doc = self.doc.inner_mut();
                doc.unactive_node();

                if event.is_primary && matches!(event.id, BlitzPointerId::Finger(_)) {
                    hover_end_event = Some(event.clone());
                }
            }
            UiEvent::PointerCancel(event) => {
                hover_node_id = self.handle_pointer_move(event);
                let mut doc = self.doc.inner_mut();
                doc.unactive_node();

                if event.is_primary && matches!(event.id, BlitzPointerId::Finger(_)) {
                    hover_end_event = Some(event.clone());
                }
            }
            UiEvent::PointerLeave(event) => {
                self.handle_pointer_leave(event);
                hover_node_id = None;
            }
            _ => {}
        };

        let target = match event {
            UiEvent::PointerMove(_) => hover_node_id,
            UiEvent::PointerUp(_) => hover_node_id,
            UiEvent::PointerDown(_) => hover_node_id,
            UiEvent::PointerCancel(_) => hover_node_id,
            UiEvent::PointerLeave(_) => hover_node_id,
            UiEvent::Wheel(_) => hover_node_id,
            UiEvent::KeyUp(_) => focussed_node_id,
            UiEvent::KeyDown(_) => focussed_node_id,
            UiEvent::Ime(_) => focussed_node_id,
            UiEvent::Paste(_) => focussed_node_id,
            UiEvent::AppleStandardKeybinding(_) => focussed_node_id,
        };
        // Fall back to the root element. A document without a root element (e.g. an
        // empty iframe sub-document) has no event target, so there is nothing to do.
        let Some(target) = target.or_else(|| self.doc.inner().try_root_element().map(|el| el.id))
        else {
            return;
        };

        match event {
            UiEvent::PointerMove(data) => {
                self.handle_input_pointer_event(target, DomEventData::PointerMove(data))
            }
            UiEvent::PointerUp(data) => {
                self.handle_input_pointer_event(target, DomEventData::PointerUp(data))
            }
            UiEvent::PointerDown(data) => {
                self.handle_input_pointer_event(target, DomEventData::PointerDown(data))
            }
            UiEvent::PointerCancel(data) => {
                self.handle_input_pointer_event(target, DomEventData::PointerCancel(data))
            }
            // Out/leave events were dispatched by `handle_pointer_leave`
            UiEvent::PointerLeave(_) => {}
            UiEvent::Wheel(data) => self.handle_dom_event(DomEvent::new(
                target,
                DomEventData::Wheel(data),
                EventDispatchSource::Input,
            )),
            UiEvent::KeyUp(data) => self.handle_dom_event(DomEvent::new(
                target,
                DomEventData::KeyUp(data),
                EventDispatchSource::Input,
            )),
            UiEvent::KeyDown(data) => self.handle_dom_event(DomEvent::new(
                target,
                DomEventData::KeyDown(data),
                EventDispatchSource::Input,
            )),
            UiEvent::Ime(data) => self.handle_dom_event(DomEvent::new(
                target,
                DomEventData::Ime(data),
                EventDispatchSource::Input,
            )),
            UiEvent::Paste(text) => {
                super::ime::handle_ime_event(
                    &mut self.doc.inner_mut(),
                    blitz_traits::events::BlitzImeEvent::Commit(text),
                );
                self.flush_pending_events();
            }
            UiEvent::AppleStandardKeybinding(data) => {
                let mut dom_event = DomEvent::new(
                    target,
                    DomEventData::AppleStandardKeybinding(data),
                    EventDispatchSource::Input,
                );
                self.run_default_action(&mut dom_event, false);
                self.flush_pending_events();
            }
        };

        // Update document input state (hover, focus, active, etc)
        if let Some(event) = hover_end_event {
            self.handle_pointer_leave(&event);
        }

        // Update the cursor once the event (including any forwarding to a
        // sub-document) has been fully handled
        let doc = self.doc.inner();
        if !is_leave && doc.hover_state() != prev_hover {
            doc.update_cursor();
        }
    }

    pub fn handle_dom_event(&mut self, event: DomEvent) {
        self.doc.inner_mut().queue_event(event);
        self.flush_pending_events();
    }

    fn handle_input_pointer_event(&mut self, target: NodeId, data: DomEventData) {
        let event = DomEvent::new(target, data, EventDispatchSource::Input);
        self.doc.inner_mut().queue_event(event);
        self.flush_pending_events();
    }

    /// Dispatch a pointer event that comes from user input, along with its compatibility
    /// event, and then run its default action.
    fn dispatch_input_pointer_event(&mut self, mut ptr_event: DomEvent) {
        type MakeData = fn(BlitzPointerEvent) -> DomEventData;
        let (data, make_mouse_data, make_touch_data): (_, Option<MakeData>, MakeData) =
            match &ptr_event.data {
                DomEventData::PointerMove(data) => (
                    data.clone(),
                    Some(DomEventData::MouseMove),
                    DomEventData::TouchMove,
                ),
                DomEventData::PointerDown(data) => (
                    data.clone(),
                    Some(DomEventData::MouseDown),
                    DomEventData::TouchStart,
                ),
                DomEventData::PointerUp(data) => (
                    data.clone(),
                    Some(DomEventData::MouseUp),
                    DomEventData::TouchEnd,
                ),
                // `pointercancel` has no mouse-compatibility event, but does generate a
                // `touchcancel` for touch-like inputs.
                DomEventData::PointerCancel(data) => {
                    (data.clone(), None, DomEventData::TouchCancel)
                }
                _ => return self.dispatch_event(ptr_event),
            };
        let target = ptr_event.target;

        let mut event_state = EventState::default();
        event_state = self.run_handler_event(&mut ptr_event, event_state);

        // Generate the corresponding compatibility event (mouse events for the
        // mouse, touch events for fingers and pen/stylus input) and expose it to
        // script. The default action is always run on the pointer event, after the
        // compatibility event's handlers (which can also prevent it), so that the
        // shell layer and default actions remain pointer-based.
        if !event_state.is_cancelled() {
            if data.is_mouse() {
                if let Some(make_mouse_data) = make_mouse_data {
                    let mut mouse_event =
                        DomEvent::new(target, make_mouse_data(data), EventDispatchSource::Input);
                    event_state = self.run_handler_event(&mut mouse_event, event_state);
                }
            } else if data.is_finger() || data.is_pen() {
                let mut touch_event =
                    DomEvent::new(target, make_touch_data(data), EventDispatchSource::Input);
                event_state = self.run_handler_event(&mut touch_event, event_state);
            }
        }

        self.run_default_action(&mut ptr_event, event_state.is_cancelled());
    }

    fn dispatch_event(&mut self, mut event: DomEvent) {
        let event_state = self.run_handler_event(&mut event, EventState::default());
        self.run_default_action(&mut event, event_state.is_cancelled());
    }

    /// Dispatch the events queued on the document (see [`BaseDocument::queue_event`]) until
    /// none are left. Each event is handed to its handlers and then to the document, either
    /// of which may queue further events.
    ///
    /// Handlers that keep queueing events (e.g. two focus handlers which each focus the
    /// other's node) would otherwise never let this return, so it stops after
    /// `MAX_EVENTS_PER_FLUSH` events and leaves the rest queued for the next flush.
    ///
    /// [`BaseDocument::queue_event`]: crate::BaseDocument::queue_event
    pub fn flush_pending_events(&mut self) {
        const MAX_EVENTS_PER_FLUSH: usize = 1024;

        for _ in 0..MAX_EVENTS_PER_FLUSH {
            let Some(event) = self.doc.inner_mut().pop_pending_event() else {
                return;
            };
            match event.source {
                EventDispatchSource::Input => self.dispatch_input_pointer_event(event),
                EventDispatchSource::UserAgent | EventDispatchSource::Script => {
                    self.dispatch_event(event)
                }
            }
        }

        #[cfg(feature = "tracing")]
        tracing::warn!(
            "Stopped dispatching events after {MAX_EVENTS_PER_FLUSH}: handlers keep queueing more"
        );
    }

    fn adjust_element_coords(
        &self,
        target: NodeId,
        coords: &PointerCoords,
        element: &mut Point<f32>,
    ) {
        if let Some(rect) = self.doc.inner().get_client_bounding_rect(target) {
            element.x = coords.client_x - rect.x as f32;
            element.y = coords.client_y - rect.y as f32;
        }
    }

    fn run_handler_event(
        &mut self,
        event: &mut DomEvent,
        initial_event_state: EventState,
    ) -> EventState {
        let chain = if event.bubbles {
            let doc = self.doc.inner();
            doc.node_chain(event.target)
        } else {
            vec![event.target]
        };

        match &mut event.data {
            DomEventData::PointerMove(data)
            | DomEventData::PointerDown(data)
            | DomEventData::PointerUp(data)
            | DomEventData::PointerCancel(data)
            | DomEventData::PointerEnter(data)
            | DomEventData::PointerLeave(data)
            | DomEventData::PointerOver(data)
            | DomEventData::PointerOut(data)
            | DomEventData::MouseMove(data)
            | DomEventData::MouseDown(data)
            | DomEventData::MouseUp(data)
            | DomEventData::MouseEnter(data)
            | DomEventData::MouseLeave(data)
            | DomEventData::MouseOver(data)
            | DomEventData::MouseOut(data)
            | DomEventData::TouchStart(data)
            | DomEventData::TouchEnd(data)
            | DomEventData::TouchMove(data)
            | DomEventData::TouchCancel(data)
            | DomEventData::Click(data)
            | DomEventData::ContextMenu(data)
            | DomEventData::DoubleClick(data) => {
                self.adjust_element_coords(event.target, &data.coords, &mut data.element)
            }
            DomEventData::Wheel(data) => {
                self.adjust_element_coords(event.target, &data.coords, &mut data.element)
            }
            _ => {}
        }

        let mut event_state = initial_event_state;
        self.handler
            .handle_event(&chain, event, self.doc, &mut event_state);

        event_state
    }

    /// Hand an event to the document once its handlers have run. This happens for every
    /// event, including one whose default action was prevented: the document decides what,
    /// if anything, it still has to do with it.
    fn run_default_action(&mut self, event: &mut DomEvent, default_prevented: bool) {
        self.doc
            .inner_mut()
            .handle_dom_event(event, default_prevented);
    }
}
