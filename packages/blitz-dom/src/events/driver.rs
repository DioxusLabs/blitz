use crate::Document;
use crate::events::listeners::{EventListener, EventListenerId};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, DomEvent, DomEventData, EventPhase, EventState, Point,
    PointerCoords, UiEvent,
};

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
    /// The [`EventContext`] provides access to the document and allows the listener to
    /// synchronously dispatch further events (see [`EventContext::dispatch_event`] and
    /// [`EventContext::set_focus`]).
    ///
    /// This method takes `&self` (rather than `&mut self`) because event dispatch is
    /// reentrant: a listener may synchronously trigger the dispatch of further events
    /// (e.g. by changing the focus), re-entering the handler while it is already on the
    /// stack. Handlers which need mutable state should use interior mutability.
    ///
    /// [`BaseDocument::add_event_listener`]: crate::BaseDocument::add_event_listener
    fn handle_event_listener(
        &self,
        listener: EventListenerId,
        event: &mut DomEvent,
        ctx: &mut EventContext<'_>,
        event_state: &mut EventState,
    );
}

pub struct NoopEventHandler;
impl EventHandler for NoopEventHandler {
    fn handle_event_listener(
        &self,
        _listener: EventListenerId,
        _event: &mut DomEvent,
        _ctx: &mut EventContext<'_>,
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
        // Dispatch any events which were queued on the document outside of an event
        // driver (e.g. focus events generated by embedder code) before the new event
        self.flush_pending_events();

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
        self.doc.inner_mut().queue_event(event);
        self.flush_pending_events();
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
        self.flush_pending_events();
    }

    /// Dispatch the document's pending events (see [`BaseDocument::queue_event`]) in a
    /// run-to-completion loop: each event is dispatched to its listeners and (if not
    /// cancelled) has its default action run, either of which may queue further events.
    ///
    /// [`BaseDocument::queue_event`]: crate::BaseDocument::queue_event
    pub fn flush_pending_events(&mut self) {
        loop {
            let Some(mut event) = self.doc.inner_mut().pop_pending_event() else {
                break;
            };
            let event_state = self.run_handler_event(&mut event, EventState::default());
            if !event_state.is_cancelled() {
                self.run_default_action(&mut event);
            }
        }
    }

    fn run_handler_event(
        &mut self,
        event: &mut DomEvent,
        initial_event_state: EventState,
    ) -> EventState {
        dispatch_event_to_listeners(&mut *self.doc, &self.handler, event, initial_event_state)
    }

    fn run_default_action(&mut self, event: &mut DomEvent) {
        self.doc.inner_mut().handle_dom_event(event);
    }
}

/// The context passed to an [`EventHandler`] for each listener invocation.
///
/// As well as providing access to the document, the context allows a listener to
/// synchronously dispatch further events ([`dispatch_event`](Self::dispatch_event)) and to
/// synchronously move the focus ([`set_focus`](Self::set_focus)/[`clear_focus`](Self::clear_focus)),
/// with the resulting events dispatched *nested*, before the current dispatch continues
/// (matching the DOM's reentrant `dispatchEvent` and focusing steps).
pub struct EventContext<'a> {
    doc: &'a mut dyn Document,
    handler: &'a dyn EventHandler,
}

impl EventContext<'_> {
    /// Access the document
    pub fn doc(&self) -> &dyn Document {
        self.doc
    }

    /// Mutably access the document.
    ///
    /// Note: state-changing document APIs (such as [`BaseDocument::set_focus_to`]) *queue*
    /// the events they generate, to be dispatched after the current event completes. Use
    /// the methods on this context instead for synchronous (nested) dispatch.
    ///
    /// [`BaseDocument::set_focus_to`]: crate::BaseDocument::set_focus_to
    pub fn doc_mut(&mut self) -> &mut dyn Document {
        self.doc
    }

    /// Synchronously dispatch an event: the event is dispatched to the listeners along its
    /// propagation path and then (if not cancelled) has its default action run, all before
    /// this method returns. This is the equivalent of the DOM `dispatchEvent` API and may
    /// be called reentrantly from within a listener.
    ///
    /// Returns `false` if the event was cancelled with `prevent_default`.
    pub fn dispatch_event(&mut self, mut event: DomEvent) -> bool {
        let event_state = dispatch_event_to_listeners(
            &mut *self.doc,
            self.handler,
            &mut event,
            EventState::default(),
        );
        let cancelled = event_state.is_cancelled();
        if !cancelled {
            self.doc.inner_mut().handle_dom_event(&mut event);
        }
        !cancelled
    }

    /// Focus the given node, synchronously dispatching the blur/focusout events for the
    /// previously focussed node and the focus/focusin events for the newly focussed node
    /// (the DOM "focusing steps").
    ///
    /// Does nothing if the node is already focussed.
    pub fn set_focus(&mut self, node_id: usize) {
        self.dispatch_events_generated_by(|doc| {
            doc.set_focus_to(node_id);
        });
    }

    /// Clear the focussed node, synchronously dispatching its blur/focusout events.
    pub fn clear_focus(&mut self) {
        self.dispatch_events_generated_by(|doc| doc.clear_focus());
    }

    /// Run a document operation and synchronously dispatch exactly the events which it
    /// queued, leaving any previously queued events in the queue.
    fn dispatch_events_generated_by(&mut self, operation: impl FnOnce(&mut crate::BaseDocument)) {
        let generated_events = {
            let mut doc = self.doc.inner_mut();
            let queued_len = doc.pending_events.len();
            operation(&mut doc);
            doc.pending_events.split_off(queued_len)
        };
        for event in generated_events {
            self.dispatch_event(event);
        }
    }
}

fn adjust_element_coords(
    doc: &dyn Document,
    target: usize,
    coords: &PointerCoords,
    element: &mut Point<f32>,
) {
    if let Some(rect) = doc.inner().get_client_bounding_rect(target) {
        element.x = coords.client_x - rect.x as f32;
        element.y = coords.client_y - rect.y as f32;
    }
}

/// Dispatch an event to the listeners registered on the nodes along its propagation path,
/// implementing the DOM event dispatch algorithm: a capture phase from the root down to
/// the target, a target phase, and (for bubbling events) a bubble phase from the target
/// back up to the root.
///
/// This is a free function (rather than a method on [`EventDriver`]) so that it can also
/// be called reentrantly from within a listener via [`EventContext::dispatch_event`].
fn dispatch_event_to_listeners(
    doc: &mut dyn Document,
    handler: &dyn EventHandler,
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
            adjust_element_coords(doc, event.target, &data.coords, &mut data.element)
        }
        DomEventData::Wheel(data) => {
            adjust_element_coords(doc, event.target, &data.coords, &mut data.element)
        }
        _ => {}
    }

    let mut dispatch_state = EventState::default();

    // Fast path: skip dispatch entirely if nothing is listening for this kind of event
    let kind = event.data.kind();
    if doc.inner().event_listeners.has_listeners_for_kind(kind) {
        // Compute the propagation path (target first, root last)
        let path = doc.inner().node_chain(event.target);

        dispatch_phases(doc, handler, &path, event, &mut dispatch_state);

        // Dispatch is complete: the event is no longer being propagated
        event.phase = EventPhase::None;
        event.current_target = None;
    }

    initial_event_state.merge(&dispatch_state)
}

fn dispatch_phases(
    doc: &mut dyn Document,
    handler: &dyn EventHandler,
    path: &[usize],
    event: &mut DomEvent,
    state: &mut EventState,
) {
    // Capture phase: walk down from the root to the target (target excluded),
    // invoking capture listeners
    event.phase = EventPhase::Capturing;
    for &node_id in path[1..].iter().rev() {
        invoke_listeners_on_node(doc, handler, node_id, event, state);
        if state.propagation_is_stopped() {
            return;
        }
    }

    // Target phase: invoke the target's listeners (both capture and non-capture
    // listeners, in registration order)
    event.phase = EventPhase::AtTarget;
    invoke_listeners_on_node(doc, handler, event.target, event, state);
    if state.propagation_is_stopped() || !event.bubbles {
        return;
    }

    // Bubble phase: walk up from the target (excluded) to the root,
    // invoking non-capture listeners
    event.phase = EventPhase::Bubbling;
    for &node_id in path[1..].iter() {
        invoke_listeners_on_node(doc, handler, node_id, event, state);
        if state.propagation_is_stopped() {
            return;
        }
    }
}

/// Invoke the listeners registered on a single node which match the event's kind
/// and current propagation phase.
fn invoke_listeners_on_node(
    doc: &mut dyn Document,
    handler: &dyn EventHandler,
    node_id: usize,
    event: &mut DomEvent,
    event_state: &mut EventState,
) {
    let kind = event.data.kind();

    // Snapshot the node's listener list so that listeners added while the event is
    // being dispatched are not invoked for this event (matching DOM semantics)
    let listeners: Vec<EventListener> = doc.inner().event_listeners.listeners_for(node_id, kind);
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
        let still_registered = doc.inner().event_listeners.contains(
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
            doc.inner_mut().event_listeners.remove(
                node_id,
                kind,
                listener.id,
                listener.options.capture,
            );
        }

        let mut listener_state = EventState::default();
        let mut ctx = EventContext {
            doc: &mut *doc,
            handler,
        };
        handler.handle_event_listener(listener.id, event, &mut ctx, &mut listener_state);

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
