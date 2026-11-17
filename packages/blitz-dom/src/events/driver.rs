use crate::Document;
use crate::document::{ActivePointerRecord, CaptureTarget};
use crate::events::DragMode;
use blitz_traits::events::{
    BlitzPointerEvent, DomEvent, DomEventData, EventState, MouseEventButtons, Point, PointerCoords,
    UiEvent,
};
use blitz_traits::node_id::NodeId;
use std::collections::VecDeque;

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

    pub fn handle_pointer_move(&mut self, event: &BlitzPointerEvent) -> Option<NodeId> {
        let pointer_id = self.upsert_active_pointer(event, event.buttons);
        if event.is_mouse() && event.buttons.is_empty() {
            self.doc
                .inner_mut()
                .pending_pointer_capture_targets
                .remove(&pointer_id);
        }
        self.update_pointer_target_and_hover(pointer_id, event)
    }

    /// End hover when the pointer leaves the document, dispatching out/leave
    /// events to the previously hovered chain
    pub fn handle_pointer_leave(&mut self, event: &BlitzPointerEvent) {
        let pointer_id = self.upsert_active_pointer(event, event.buttons);
        if event.buttons.is_empty() {
            self.doc
                .inner_mut()
                .pending_pointer_capture_targets
                .remove(&pointer_id);
            self.process_pending_pointer_capture(pointer_id, event);
        }

        if self
            .doc
            .inner()
            .active_capture_node_by_dom_id(pointer_id)
            .is_some()
        {
            return;
        }

        let prev_hover_node_id = {
            let mut doc = self.doc.inner_mut();
            let prev = doc
                .hover_node_id
                .and_then(|id| doc.nearest_connected_element(id));
            doc.clear_hover();
            prev
        };

        let prev_under_pointer = {
            let mut doc = self.doc.inner_mut();
            doc.active_pointers
                .get_mut(&pointer_id)
                .and_then(|record| record.element_under_pointer.take())
                .or(prev_hover_node_id)
        };

        self.dispatch_hover_change(prev_under_pointer, None, event);

        if !event.is_mouse() {
            self.remove_active_pointer(pointer_id);
        }
    }

    fn upsert_active_pointer(
        &mut self,
        event: &BlitzPointerEvent,
        active_buttons: MouseEventButtons,
    ) -> i32 {
        let pointer_id = event.id.to_dom_pointer_id();
        let mut stored_event = event.clone();
        stored_event.buttons = active_buttons;
        stored_event.active_pointers = Default::default();

        let mut doc = self.doc.inner_mut();
        let initial_under_pointer = if event.is_mouse() {
            doc.hover_node_id
                .and_then(|id| doc.nearest_connected_element(id))
        } else {
            None
        };
        doc.active_pointers
            .entry(pointer_id)
            .and_modify(|record| {
                record.id = event.id;
                record.is_primary = event.is_primary;
                record.buttons = active_buttons;
                record.last_event = stored_event.clone();
            })
            .or_insert(ActivePointerRecord {
                id: event.id,
                is_primary: event.is_primary,
                buttons: active_buttons,
                last_event: stored_event,
                element_under_pointer: initial_under_pointer,
            });
        pointer_id
    }

    fn remove_active_pointer(&mut self, pointer_id: i32) {
        let mut doc = self.doc.inner_mut();
        doc.active_pointers.remove(&pointer_id);
        doc.pending_pointer_capture_targets.remove(&pointer_id);
        doc.pointer_capture_targets.remove(&pointer_id);
    }

    fn resolve_target_or_root(&self, target: Option<NodeId>) -> Option<NodeId> {
        target.or_else(|| self.doc.inner().try_root_element().map(|el| el.id))
    }

    fn set_element_under_pointer(
        &mut self,
        pointer_id: i32,
        next_target: Option<NodeId>,
        event: &BlitzPointerEvent,
    ) {
        let (prev_target, next_target) = {
            let mut doc = self.doc.inner_mut();
            let next_target = next_target.and_then(|id| doc.nearest_connected_element(id));
            let Some(record) = doc.active_pointers.get_mut(&pointer_id) else {
                return;
            };
            let prev = record.element_under_pointer;
            if prev == next_target {
                return;
            }
            record.element_under_pointer = next_target;
            (prev, next_target)
        };

        self.dispatch_hover_change(prev_target, next_target, event);
    }

    fn process_pending_pointer_capture(&mut self, pointer_id: i32, event: &BlitzPointerEvent) {
        let (old_capture, initial_pending) = {
            let mut doc = self.doc.inner_mut();
            let old = doc.pointer_capture_targets.get(&pointer_id).copied();
            let pending = doc
                .pending_pointer_capture_targets
                .get(&pointer_id)
                .copied();

            match (old, pending) {
                (None, None) => return,
                (
                    Some(CaptureTarget::Connected {
                        target: old_target,
                        explicit: old_explicit,
                    }),
                    Some(pending_capture),
                ) if old_target == pending_capture.target => {
                    if old_explicit != pending_capture.explicit {
                        doc.pointer_capture_targets.insert(
                            pointer_id,
                            CaptureTarget::Connected {
                                target: old_target,
                                explicit: pending_capture.explicit,
                            },
                        );
                    }
                    return;
                }
                _ => (old, pending),
            }
        };

        if let Some(old) = old_capture {
            let lost_target = {
                let mut doc = self.doc.inner_mut();
                doc.pointer_capture_targets.remove(&pointer_id);
                match old {
                    CaptureTarget::Connected { target, .. } if doc.is_connected_element(target) => {
                        target
                    }
                    _ => doc.root_node_id,
                }
            };
            self.handle_dom_event(DomEvent::new(
                lost_target,
                DomEventData::LostPointerCapture(event.clone()),
            ));
        }

        if initial_pending.is_none() {
            return;
        }

        let current_pending = {
            let doc = self.doc.inner();
            doc.pending_pointer_capture_targets
                .get(&pointer_id)
                .copied()
                .filter(|pending| doc.is_connected_element(pending.target))
        };

        let Some(captured) = current_pending else {
            return;
        };

        if event.is_primary || event.is_mouse() {
            self.doc.inner_mut().set_hover_to_captured_node(
                captured.target,
                event.page_x(),
                event.page_y(),
            );
        }
        self.set_element_under_pointer(pointer_id, Some(captured.target), event);

        self.doc.inner_mut().pointer_capture_targets.insert(
            pointer_id,
            CaptureTarget::Connected {
                target: captured.target,
                explicit: captured.explicit,
            },
        );

        self.handle_dom_event(DomEvent::new(
            captured.target,
            DomEventData::GotPointerCapture(event.clone()),
        ));

        let disconnected_during_got = {
            let mut doc = self.doc.inner_mut();
            let disconnected = matches!(
                doc.pointer_capture_targets.get(&pointer_id),
                Some(CaptureTarget::DisconnectedPendingLostCapture)
            ) || !doc.is_connected_element(captured.target);
            if disconnected {
                doc.pointer_capture_targets.remove(&pointer_id);
                Some(doc.root_node_id)
            } else {
                None
            }
        };

        if let Some(root_node_id) = disconnected_during_got {
            self.handle_dom_event(DomEvent::new(
                root_node_id,
                DomEventData::LostPointerCapture(event.clone()),
            ));
        }
    }

    fn update_pointer_target_and_hover(
        &mut self,
        pointer_id: i32,
        event: &BlitzPointerEvent,
    ) -> Option<NodeId> {
        self.process_pending_pointer_capture(pointer_id, event);

        let captured_id = self.doc.inner().active_capture_node_by_dom_id(pointer_id);
        if let Some(captured_id) = captured_id {
            if event.is_primary || event.is_mouse() {
                self.doc.inner_mut().set_hover_to_captured_node(
                    captured_id,
                    event.page_x(),
                    event.page_y(),
                );
            }
            self.set_element_under_pointer(pointer_id, Some(captured_id), event);
            return Some(captured_id);
        }

        let hit_target = if event.is_primary || event.is_mouse() {
            let mut doc = self.doc.inner_mut();
            doc.set_hover_to(event.page_x(), event.page_y());
            doc.hover_node_id
        } else {
            let doc = self.doc.inner();
            doc.hit(event.page_x(), event.page_y())
                .and_then(|hit| doc.nearest_non_anonymous_ancestor(hit.node_id))
        };

        self.set_element_under_pointer(pointer_id, hit_target, event);
        hit_target
    }

    fn dispatch_hover_change(
        &mut self,
        prev_hover_node_id: Option<NodeId>,
        hover_node_id: Option<NodeId>,
        event: &BlitzPointerEvent,
    ) {
        if prev_hover_node_id == hover_node_id {
            return;
        }

        let doc = self.doc.inner();
        let prev_hover_node_id = prev_hover_node_id.filter(|&id| doc.nodes.contains_key(id));
        let hover_node_id = hover_node_id.filter(|&id| doc.nodes.contains_key(id));
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
                if !self.doc.inner().nodes.contains_key(*node_id) {
                    continue;
                }
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

        if let Some(target) = hover_node_id
            && self.doc.inner().nodes.contains_key(target)
        {
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
                if !self.doc.inner().nodes.contains_key(*node_id) {
                    continue;
                }
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
    }

    pub fn handle_ui_event(&mut self, event: UiEvent) {
        let (hover_node_id, focussed_node_id, prev_hover) = {
            let doc = self.doc.inner();
            (doc.hover_node_id, doc.focus_node_id, doc.hover_state())
        };

        // A document the pointer has left has no say over the cursor
        let is_leave = matches!(event, UiEvent::PointerLeave(_));

        match event {
            UiEvent::PointerMove(data) => {
                let effective_target = self.handle_pointer_move(&data);
                if let Some(target) = self.resolve_target_or_root(effective_target) {
                    self.handle_pointer_event(
                        target,
                        data,
                        DomEventData::PointerMove,
                        Some(DomEventData::MouseMove),
                        DomEventData::TouchMove,
                    );
                }
            }
            UiEvent::PointerDown(mut data) => {
                let trigger_mask = MouseEventButtons::from(data.button);
                let other_held = if data.is_finger() {
                    MouseEventButtons::None
                } else {
                    data.buttons.difference(trigger_mask)
                };
                let is_chorded = !other_held.is_empty();
                let active_buttons = if data.buttons.is_empty() {
                    trigger_mask
                } else {
                    data.buttons | trigger_mask
                };
                data.buttons = active_buttons;

                let pointer_id = self.upsert_active_pointer(&data, active_buttons);
                let effective_target = self.update_pointer_target_and_hover(pointer_id, &data);
                let resolved_target = self.resolve_target_or_root(effective_target);

                if !is_chorded {
                    let mut doc = self.doc.inner_mut();
                    doc.active_node();
                    doc.set_mousedown_node_id(resolved_target);
                    if data.is_finger()
                        && let Some(target_id) = effective_target
                    {
                        doc.set_implicit_pointer_capture(target_id, pointer_id);
                    }
                }

                if let Some(target) = resolved_target {
                    if is_chorded {
                        self.handle_pointer_event(
                            target,
                            data,
                            DomEventData::PointerMove,
                            Some(DomEventData::MouseDown),
                            DomEventData::TouchMove,
                        );
                    } else {
                        self.handle_pointer_event(
                            target,
                            data,
                            DomEventData::PointerDown,
                            Some(DomEventData::MouseDown),
                            DomEventData::TouchStart,
                        );
                    }
                }
            }
            UiEvent::PointerUp(mut data) => {
                let trigger_mask = MouseEventButtons::from(data.button);
                let other_held = if data.is_finger() {
                    MouseEventButtons::None
                } else {
                    data.buttons.difference(trigger_mask)
                };
                let is_chorded = !other_held.is_empty();
                data.buttons = other_held;

                let pointer_id = self.upsert_active_pointer(&data, other_held);
                let effective_target = self.update_pointer_target_and_hover(pointer_id, &data);

                if is_chorded {
                    if let Some(target) = self.resolve_target_or_root(effective_target) {
                        self.handle_pointer_event(
                            target,
                            data,
                            DomEventData::PointerMove,
                            Some(DomEventData::MouseUp),
                            DomEventData::TouchMove,
                        );
                    }
                } else {
                    self.doc.inner_mut().unactive_node();

                    if let Some(target) = self.resolve_target_or_root(effective_target) {
                        let mut ptr_event =
                            DomEvent::new(target, DomEventData::PointerUp(data.clone()));
                        let mut event_state =
                            self.run_handler_event(&mut ptr_event, EventState::default());

                        if !event_state.is_cancelled() {
                            if data.is_mouse() {
                                let mut mouse_event =
                                    DomEvent::new(target, DomEventData::MouseUp(data.clone()));
                                event_state = self.run_handler_event(&mut mouse_event, event_state);
                            } else if data.is_finger() || data.is_pen() {
                                let mut touch_event =
                                    DomEvent::new(target, DomEventData::TouchEnd(data.clone()));
                                event_state = self.run_handler_event(&mut touch_event, event_state);
                            }
                        }

                        let captured_click_target =
                            self.doc.inner().active_capture_node_by_dom_id(pointer_id);
                        self.doc
                            .inner_mut()
                            .pending_pointer_capture_targets
                            .remove(&pointer_id);
                        self.process_pending_pointer_capture(pointer_id, &data);

                        {
                            let mut doc = self.doc.inner_mut();
                            let mousedown_node_id = doc.mousedown_node_id.take();
                            let click_target = if let Some(captured_id) = captured_click_target {
                                mousedown_node_id
                                    .is_some()
                                    .then_some(captured_id)
                                    .filter(|&id| doc.is_connected_element(id))
                            } else if let Some(down_id) = mousedown_node_id {
                                let release_id = doc
                                    .active_pointers
                                    .get(&pointer_id)
                                    .and_then(|record| record.element_under_pointer)
                                    .or(effective_target)
                                    .or(Some(target));
                                release_id
                                    .and_then(|up_id| doc.common_element_ancestor(down_id, up_id))
                            } else {
                                None
                            };
                            doc.pending_click_target = Some(click_target);
                        }

                        if !event_state.is_cancelled() {
                            self.run_default_action(&mut ptr_event);
                        } else {
                            let mut doc = self.doc.inner_mut();
                            doc.pending_click_target = None;
                            doc.drag_mode.take();
                        }
                        self.process_queue();
                    } else {
                        self.doc
                            .inner_mut()
                            .pending_pointer_capture_targets
                            .remove(&pointer_id);
                        self.process_pending_pointer_capture(pointer_id, &data);
                        let mut doc = self.doc.inner_mut();
                        doc.mousedown_node_id = None;
                        doc.drag_mode.take();
                    }

                    if data.is_finger() {
                        if data.is_primary {
                            self.doc.inner_mut().clear_hover();
                        }
                        self.set_element_under_pointer(pointer_id, None, &data);
                        self.remove_active_pointer(pointer_id);
                    } else {
                        self.update_pointer_target_and_hover(pointer_id, &data);
                    }
                }
            }
            UiEvent::PointerCancel(mut data) => {
                data.buttons = MouseEventButtons::None;
                let pointer_id = self.upsert_active_pointer(&data, MouseEventButtons::None);
                let effective_target = self.update_pointer_target_and_hover(pointer_id, &data);

                {
                    let mut doc = self.doc.inner_mut();
                    doc.unactive_node();
                    doc.drag_mode = DragMode::None;
                    doc.mousedown_node_id = None;
                }

                if let Some(target) = self.resolve_target_or_root(effective_target) {
                    // `pointercancel` has no mouse-compatibility event, but does
                    // generate a `touchcancel` for touch-like inputs.
                    self.handle_pointer_event(
                        target,
                        data.clone(),
                        DomEventData::PointerCancel,
                        None::<fn(BlitzPointerEvent) -> DomEventData>,
                        DomEventData::TouchCancel,
                    );
                }

                self.doc
                    .inner_mut()
                    .pending_pointer_capture_targets
                    .remove(&pointer_id);
                self.process_pending_pointer_capture(pointer_id, &data);

                if data.is_finger() {
                    if data.is_primary {
                        self.doc.inner_mut().clear_hover();
                    }
                    self.set_element_under_pointer(pointer_id, None, &data);
                    self.remove_active_pointer(pointer_id);
                }
            }
            UiEvent::PointerLeave(data) => {
                self.handle_pointer_leave(&data);
            }
            UiEvent::Wheel(data) => {
                if let Some(target) = self.resolve_target_or_root(hover_node_id) {
                    self.handle_dom_event(DomEvent::new(target, DomEventData::Wheel(data)));
                }
            }
            UiEvent::KeyUp(data) => {
                if let Some(target) = self.resolve_target_or_root(focussed_node_id) {
                    self.handle_dom_event(DomEvent::new(target, DomEventData::KeyUp(data)));
                }
            }
            UiEvent::KeyDown(data) => {
                if let Some(target) = self.resolve_target_or_root(focussed_node_id) {
                    self.handle_dom_event(DomEvent::new(target, DomEventData::KeyDown(data)));
                }
            }
            UiEvent::Ime(data) => {
                if let Some(target) = self.resolve_target_or_root(focussed_node_id) {
                    self.handle_dom_event(DomEvent::new(target, DomEventData::Ime(data)));
                }
            }
            UiEvent::Paste(text) => {
                if self.resolve_target_or_root(focussed_node_id).is_some() {
                    super::ime::handle_ime_event(
                        &mut self.doc.inner_mut(),
                        blitz_traits::events::BlitzImeEvent::Commit(text),
                        |event| self.queue.push_back(event),
                    );
                    self.process_queue();
                }
            }
            UiEvent::AppleStandardKeybinding(data) => {
                if let Some(target) = self.resolve_target_or_root(focussed_node_id) {
                    let mut dom_event =
                        DomEvent::new(target, DomEventData::AppleStandardKeybinding(data));
                    self.run_default_action(&mut dom_event);
                    self.process_queue();
                }
            }
        }

        // Update the cursor once the event (including any forwarding to a
        // sub-document) has been fully handled
        let doc = self.doc.inner();
        if !is_leave && doc.hover_state() != prev_hover {
            doc.update_cursor();
        }
    }

    pub fn handle_dom_event(&mut self, event: DomEvent) {
        self.queue.push_back(event);
        self.process_queue();
    }

    fn handle_pointer_event(
        &mut self,
        target: NodeId,
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
        let chain = {
            let doc = self.doc.inner();
            if !doc.nodes.contains_key(event.target) {
                return initial_event_state;
            }
            if event.bubbles {
                doc.node_chain(event.target)
            } else {
                vec![event.target]
            }
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
            | DomEventData::GotPointerCapture(data)
            | DomEventData::LostPointerCapture(data)
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

    fn run_default_action(&mut self, event: &mut DomEvent) {
        let mut doc = self.doc.inner_mut();
        doc.handle_dom_event(event, |new_evt| self.queue.push_back(new_evt));
    }
}
