use crate::Document;
use crate::events::listeners::{EventListener, EventListenerId};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, DomEvent, DomEventData, EventPhase, EventState, Point,
    PointerCoords, UiEvent,
};
use std::collections::VecDeque;

pub trait EventHandler {
    /// Invoked once for each registered event listener that matches a dispatched event.
    ///
    /// Blitz does not store listener callbacks itself: listeners are registered on nodes as
    /// opaque [`EventListenerId`]s (see [`BaseDocument::add_event_listener`]), and the
    /// implementor of this trait is responsible for mapping each id back to actual behaviour
    /// (a Rust closure, a Dioxus vdom handler, a script function, etc).
    ///
    /// `event.current_target` and `event.phase` describe the point in the propagation path
    /// at which the listener is being invoked. The listener can influence further processing
    /// of the event (`prevent_default`, `stop_propagation`, `stop_immediate_propagation`)
    /// through `event_state`.
    ///
    /// [`BaseDocument::add_event_listener`]: crate::BaseDocument::add_event_listener
    fn handle_event_listener(
        &mut self,
        listener: EventListenerId,
        event: &mut DomEvent,
        doc: &mut dyn Document,
        event_state: &mut EventState,
    );
}

pub struct NoopEventHandler;
impl EventHandler for NoopEventHandler {
    fn handle_event_listener(
        &mut self,
        _listener: EventListenerId,
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
    queue: VecDeque<DomEvent>,
}

impl<'doc, Handler: EventHandler> EventDriver<'doc, Handler> {
    pub fn new(doc: &'doc mut dyn Document, handler: Handler) -> Self {
        EventDriver {
            doc,
            handler,
            queue: VecDeque::with_capacity(4),
        }
    }

    pub fn handle_pointer_move(&mut self, event: &BlitzPointerEvent) -> Option<usize> {
        let mut doc = self.doc.inner_mut();

        let prev_hover_node_id = doc.hover_node_id;
        let changed = doc.set_hover_to(event.page_x(), event.page_y());
        let hover_node_id = doc.hover_node_id;

        drop(doc);

        if !changed {
            return prev_hover_node_id;
        }

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
            ));
            if is_mouse {
                self.handle_dom_event(DomEvent::new(target, DomEventData::MouseOut(event.clone())));
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
                ));
                if is_mouse {
                    self.handle_dom_event(DomEvent::new(
                        *node_id,
                        DomEventData::MouseLeave(event.clone()),
                    ));
                }
            }
        }

        if let Some(target) = hover_node_id {
            self.handle_dom_event(DomEvent::new(
                target,
                DomEventData::PointerOver(event.clone()),
            ));

            if is_mouse {
                self.handle_dom_event(DomEvent::new(
                    target,
                    DomEventData::MouseOver(event.clone()),
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
                ));

                if is_mouse {
                    self.handle_dom_event(DomEvent::new(
                        *node_id,
                        DomEventData::MouseEnter(event.clone()),
                    ));
                }
            }
        }

        hover_node_id
    }

    pub fn handle_ui_event(&mut self, event: UiEvent) {
        let doc = self.doc.inner();

        let mut should_clear_hover = false;
        let mut hover_node_id = doc.hover_node_id;
        let focussed_node_id = doc.focus_node_id;
        drop(doc);

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
                    should_clear_hover = true;
                }
            }
            UiEvent::PointerCancel(event) => {
                hover_node_id = self.handle_pointer_move(event);
                let mut doc = self.doc.inner_mut();
                doc.unactive_node();

                if event.is_primary && matches!(event.id, BlitzPointerId::Finger(_)) {
                    should_clear_hover = true;
                }
            }
            _ => {}
        };

        let target = match event {
            UiEvent::PointerMove(_) => hover_node_id,
            UiEvent::PointerUp(_) => hover_node_id,
            UiEvent::PointerDown(_) => hover_node_id,
            UiEvent::PointerCancel(_) => hover_node_id,
            UiEvent::Wheel(_) => hover_node_id,
            UiEvent::KeyUp(_) => focussed_node_id,
            UiEvent::KeyDown(_) => focussed_node_id,
            UiEvent::Ime(_) => focussed_node_id,
            UiEvent::AppleStandardKeybinding(_) => focussed_node_id,
        };
        let target = target.unwrap_or_else(|| self.doc.inner().root_element().id);

        match event {
            UiEvent::PointerMove(data) => {
                self.handle_pointer_event(
                    target,
                    data,
                    DomEventData::PointerMove,
                    Some(DomEventData::MouseMove),
                    DomEventData::TouchMove,
                );
            }
            UiEvent::PointerUp(data) => {
                self.handle_pointer_event(
                    target,
                    data,
                    DomEventData::PointerUp,
                    Some(DomEventData::MouseUp),
                    DomEventData::TouchEnd,
                );
            }
            UiEvent::PointerDown(data) => {
                self.handle_pointer_event(
                    target,
                    data,
                    DomEventData::PointerDown,
                    Some(DomEventData::MouseDown),
                    DomEventData::TouchStart,
                );
            }
            UiEvent::PointerCancel(data) => {
                // `pointercancel` has no mouse-compatibility event, but does
                // generate a `touchcancel` for touch-like inputs.
                self.handle_pointer_event(
                    target,
                    data,
                    DomEventData::PointerCancel,
                    None::<fn(BlitzPointerEvent) -> DomEventData>,
                    DomEventData::TouchCancel,
                );
            }
            UiEvent::Wheel(data) => {
                self.handle_dom_event(DomEvent::new(target, DomEventData::Wheel(data)))
            }
            UiEvent::KeyUp(data) => {
                self.handle_dom_event(DomEvent::new(target, DomEventData::KeyUp(data)))
            }
            UiEvent::KeyDown(data) => {
                self.handle_dom_event(DomEvent::new(target, DomEventData::KeyDown(data)))
            }
            UiEvent::Ime(data) => {
                self.handle_dom_event(DomEvent::new(target, DomEventData::Ime(data)))
            }
            UiEvent::AppleStandardKeybinding(data) => {
                let mut dom_event =
                    DomEvent::new(target, DomEventData::AppleStandardKeybinding(data));
                self.run_default_action(&mut dom_event);
            }
        };

        // Update document input state (hover, focus, active, etc)
        if should_clear_hover {
            self.doc.inner_mut().clear_hover();
        }
    }

    pub fn handle_dom_event(&mut self, event: DomEvent) {
        self.queue.push_back(event);
        self.process_queue();
    }

    fn handle_pointer_event(
        &mut self,
        target: usize,
        data: BlitzPointerEvent,
        make_ptr_data: impl FnOnce(BlitzPointerEvent) -> DomEventData,
        make_mouse_data: Option<impl FnOnce(BlitzPointerEvent) -> DomEventData>,
        make_touch_data: impl FnOnce(BlitzPointerEvent) -> DomEventData,
    ) {
        let mut ptr_event = DomEvent::new(target, make_ptr_data(data.clone()));
        let mut event_state = EventState::default();
        event_state = self.run_handler_event(&mut ptr_event, event_state);

        // Generate the corresponding compatibility event (mouse events for the
        // mouse, touch events for fingers and pen/stylus input) and expose it to
        // script. The default action is always run on the pointer event so that
        // the shell layer and default actions remain pointer-based.
        //
        // `pointercancel` has no mouse equivalent, so `make_mouse_data` is `None`
        // in that case and no mouse event is generated.
        if !event_state.is_cancelled() {
            if data.is_mouse() {
                if let Some(make_mouse_data) = make_mouse_data {
                    let mut mouse_event = DomEvent::new(target, make_mouse_data(data));
                    event_state = self.run_handler_event(&mut mouse_event, event_state);
                }
            } else if data.is_finger() || data.is_pen() {
                let mut touch_event = DomEvent::new(target, make_touch_data(data));
                event_state = self.run_handler_event(&mut touch_event, event_state);
            }
        }

        if !event_state.is_cancelled() {
            self.run_default_action(&mut ptr_event);
        }
        self.process_queue();
    }

    fn process_queue(&mut self) {
        while let Some(mut event) = self.queue.pop_front() {
            let event_state = self.run_handler_event(&mut event, EventState::default());
            if !event_state.is_cancelled() {
                self.run_default_action(&mut event);
            }
        }
    }

    fn adjust_element_coords(
        &self,
        target: usize,
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

        let mut dispatch_state = EventState::default();
        self.dispatch_event_to_listeners(event, &mut dispatch_state);
        initial_event_state.merge(&dispatch_state)
    }

    /// Dispatch an event to the listeners registered on the nodes along its propagation path,
    /// implementing the DOM event dispatch algorithm: a capture phase from the root down to
    /// the target, a target phase, and (for bubbling events) a bubble phase from the target
    /// back up to the root.
    fn dispatch_event_to_listeners(&mut self, event: &mut DomEvent, event_state: &mut EventState) {
        // Fast path: skip dispatch entirely if nothing is listening for this kind of event
        let kind = event.data.kind();
        if !self
            .doc
            .inner()
            .event_listeners
            .has_listeners_for_kind(kind)
        {
            return;
        }

        // Compute the propagation path (target first, root last)
        let path = self.doc.inner().node_chain(event.target);

        self.dispatch_phases(&path, event, event_state);

        // Dispatch is complete: the event is no longer being propagated
        event.phase = EventPhase::None;
        event.current_target = None;
    }

    fn dispatch_phases(&mut self, path: &[usize], event: &mut DomEvent, state: &mut EventState) {
        // Capture phase: walk down from the root to the target (target excluded),
        // invoking capture listeners
        event.phase = EventPhase::Capturing;
        for &node_id in path[1..].iter().rev() {
            self.invoke_listeners_on_node(node_id, event, state);
            if state.propagation_is_stopped() {
                return;
            }
        }

        // Target phase: invoke the target's listeners (both capture and non-capture
        // listeners, in registration order)
        event.phase = EventPhase::AtTarget;
        self.invoke_listeners_on_node(event.target, event, state);
        if state.propagation_is_stopped() || !event.bubbles {
            return;
        }

        // Bubble phase: walk up from the target (excluded) to the root,
        // invoking non-capture listeners
        event.phase = EventPhase::Bubbling;
        for &node_id in path[1..].iter() {
            self.invoke_listeners_on_node(node_id, event, state);
            if state.propagation_is_stopped() {
                return;
            }
        }
    }

    /// Invoke the listeners registered on a single node which match the event's kind
    /// and current propagation phase.
    fn invoke_listeners_on_node(
        &mut self,
        node_id: usize,
        event: &mut DomEvent,
        event_state: &mut EventState,
    ) {
        let kind = event.data.kind();

        // Snapshot the node's listener list so that listeners added while the event is
        // being dispatched are not invoked for this event (matching DOM semantics)
        let listeners: Vec<EventListener> = self
            .doc
            .inner()
            .event_listeners
            .listeners_for(node_id, kind);
        if listeners.is_empty() {
            return;
        }

        event.current_target = Some(node_id);

        for listener in listeners {
            let phase_matches = match event.phase {
                EventPhase::Capturing => listener.options.capture,
                // At the target both capture and non-capture listeners are invoked
                EventPhase::AtTarget => true,
                EventPhase::Bubbling => !listener.options.capture,
                EventPhase::None => false,
            };
            if !phase_matches {
                continue;
            }

            // Skip listeners that were removed by an earlier listener during this dispatch
            let still_registered = self.doc.inner().event_listeners.contains(
                node_id,
                kind,
                listener.id,
                listener.options.capture,
            );
            if !still_registered {
                continue;
            }

            // `once` listeners are removed *before* they are invoked so that they can
            // re-register themselves without being immediately deregistered
            if listener.options.once {
                self.doc.inner_mut().event_listeners.remove(
                    node_id,
                    kind,
                    listener.id,
                    listener.options.capture,
                );
            }

            let mut listener_state = EventState::default();
            self.handler
                .handle_event_listener(listener.id, event, self.doc, &mut listener_state);

            // A listener may only cancel an event if the event is cancelable and the
            // listener was not registered as `passive`
            if listener_state.is_cancelled() && event.cancelable && !listener.options.passive {
                event_state.prevent_default();
            }
            if listener_state.propagation_is_stopped() {
                event_state.stop_propagation();
            }
            if listener_state.redraw_is_requested() {
                event_state.request_redraw();
            }
            if listener_state.immediate_propagation_is_stopped() {
                event_state.stop_immediate_propagation();
                break;
            }
        }
    }

    fn run_default_action(&mut self, event: &mut DomEvent) {
        let mut doc = self.doc.inner_mut();
        doc.handle_dom_event(event, |new_evt| self.queue.push_back(new_evt));
    }
}
