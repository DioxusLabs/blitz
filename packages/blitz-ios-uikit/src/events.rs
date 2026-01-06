//! Event bridging from UIKit to blitz-dom
//!
//! This module handles converting UIKit touch/gesture events to blitz-dom events
//! and dispatching them through the event system.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use blitz_traits::events::{
    BlitzInputEvent, BlitzPointerId, BlitzPointerEvent, DomEvent, DomEventData, MouseEventButton,
    MouseEventButtons, UiEvent,
};
use keyboard_types::Modifiers;

/// Sender for events from UIKit to blitz-dom.
///
/// UIKit views use this to queue events that will be processed by the document.
/// This is cheap to clone as it uses Rc internally.
#[derive(Default, Clone)]
pub struct EventSender {
    /// Queue of pending events (shared via Rc)
    events: Rc<RefCell<VecDeque<UiEvent>>>,
}

impl EventSender {
    /// Create a new event sender.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a mouse down event.
    pub fn send_mouse_down(&self, x: f32, y: f32, finger_id: u64) {
        let event = UiEvent::MouseDown(BlitzPointerEvent {
            id: BlitzPointerId::Finger(finger_id),
            is_primary: finger_id == 0,
            x,
            y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
            button: MouseEventButton::Main,
            buttons: MouseEventButtons::Primary,
            mods: Modifiers::empty(),
        });
        self.events.borrow_mut().push_back(event);
    }

    /// Queue a mouse up event.
    pub fn send_mouse_up(&self, x: f32, y: f32, finger_id: u64) {
        let event = UiEvent::MouseUp(BlitzPointerEvent {
            id: BlitzPointerId::Finger(finger_id),
            is_primary: finger_id == 0,
            x,
            y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
            button: MouseEventButton::Main,
            buttons: MouseEventButtons::None,
            mods: Modifiers::empty(),
        });
        self.events.borrow_mut().push_back(event);
    }

    /// Queue a mouse move event.
    pub fn send_mouse_move(&self, x: f32, y: f32, finger_id: u64) {
        let event = UiEvent::MouseMove(BlitzPointerEvent {
            id: BlitzPointerId::Finger(finger_id),
            is_primary: finger_id == 0,
            x,
            y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
            button: MouseEventButton::Main,
            buttons: MouseEventButtons::Primary,
            mods: Modifiers::empty(),
        });
        self.events.borrow_mut().push_back(event);
    }

    /// Drain all pending events.
    pub fn drain_events(&self) -> Vec<UiEvent> {
        self.events.borrow_mut().drain(..).collect()
    }

    /// Check if there are pending events.
    pub fn has_pending_events(&self) -> bool {
        !self.events.borrow().is_empty()
    }
}

/// Create a click DOM event for a node.
pub fn create_click_event(node_id: usize, x: f32, y: f32) -> DomEvent {
    DomEvent::new(
        node_id,
        DomEventData::Click(BlitzPointerEvent {
            id: BlitzPointerId::Finger(0),
            is_primary: true,
            x,
            y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
            button: MouseEventButton::Main,
            buttons: MouseEventButtons::None,
            mods: Modifiers::empty(),
        }),
    )
}

/// Create an input DOM event for a node.
pub fn create_input_event(node_id: usize, value: String) -> DomEvent {
    DomEvent::new(node_id, DomEventData::Input(BlitzInputEvent { value }))
}

/// Convert touch coordinates from UIKit to blitz-dom coordinate space.
///
/// UIKit uses points (logical pixels), blitz-dom uses CSS pixels.
/// The scale factor converts between them.
pub fn convert_touch_coordinates(x: f64, y: f64, scale: f64, offset_x: f64, offset_y: f64) -> (f32, f32) {
    let css_x = (x - offset_x) / scale;
    let css_y = (y - offset_y) / scale;
    (css_x as f32, css_y as f32)
}
