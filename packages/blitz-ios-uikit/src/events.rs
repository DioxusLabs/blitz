//! Event bridging from UIKit to blitz-dom
//!
//! This module handles converting UIKit touch/gesture events to blitz-dom events
//! and dispatching them through the event system.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Mutex;

use blitz_traits::events::{
    BlitzFocusEvent, BlitzInputEvent, BlitzPointerId, BlitzPointerEvent, DomEvent, DomEventData,
    MouseEventButton, MouseEventButtons, UiEvent,
};
use keyboard_types::Modifiers;

// =============================================================================
// Global Input Event Queue
// =============================================================================

/// Thread-safe queue for input events from native UIKit controls.
/// This allows objc2 code to queue events that will be processed by the Rust event loop.
static INPUT_EVENT_QUEUE: Mutex<VecDeque<InputEvent>> = Mutex::new(VecDeque::new());

/// An input event from a native UIKit control.
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// Button/element clicked
    Click { node_id: usize },
    /// Text input changed
    TextChanged { node_id: usize, value: String },
    /// Input gained focus
    FocusGained { node_id: usize },
    /// Input lost focus
    FocusLost { node_id: usize },
}

/// Queue a click event from a UIButton or other tappable element.
pub fn queue_click(node_id: usize) {
    if let Ok(mut queue) = INPUT_EVENT_QUEUE.lock() {
        println!("[InputEvent] Click node_id={}", node_id);
        queue.push_back(InputEvent::Click { node_id });
    }
}

/// Queue a text change event from a UITextField.
pub fn queue_text_changed(node_id: usize, value: String) {
    if let Ok(mut queue) = INPUT_EVENT_QUEUE.lock() {
        println!("[InputEvent] TextChanged node_id={} value={:?}", node_id, value);
        queue.push_back(InputEvent::TextChanged { node_id, value });
    }
}

/// Queue a focus gained event.
pub fn queue_focus_gained(node_id: usize) {
    if let Ok(mut queue) = INPUT_EVENT_QUEUE.lock() {
        println!("[InputEvent] FocusGained node_id={}", node_id);
        queue.push_back(InputEvent::FocusGained { node_id });
    }
}

/// Queue a focus lost event.
pub fn queue_focus_lost(node_id: usize) {
    if let Ok(mut queue) = INPUT_EVENT_QUEUE.lock() {
        println!("[InputEvent] FocusLost node_id={}", node_id);
        queue.push_back(InputEvent::FocusLost { node_id });
    }
}

/// Drain all pending input events.
pub fn drain_input_events() -> Vec<InputEvent> {
    if let Ok(mut queue) = INPUT_EVENT_QUEUE.lock() {
        queue.drain(..).collect()
    } else {
        Vec::new()
    }
}

/// Check if there are pending input events without draining them.
pub fn has_pending_input_events() -> bool {
    if let Ok(queue) = INPUT_EVENT_QUEUE.lock() {
        !queue.is_empty()
    } else {
        false
    }
}

/// Convert an InputEvent to a DomEvent.
pub fn input_event_to_dom_event(event: InputEvent) -> DomEvent {
    match event {
        InputEvent::Click { node_id } => {
            // Create a click event with dummy pointer data
            // The actual position doesn't matter for native button clicks
            DomEvent::new(
                node_id,
                DomEventData::Click(BlitzPointerEvent {
                    id: BlitzPointerId::Finger(0),
                    is_primary: true,
                    x: 0.0,
                    y: 0.0,
                    screen_x: 0.0,
                    screen_y: 0.0,
                    client_x: 0.0,
                    client_y: 0.0,
                    button: MouseEventButton::Main,
                    buttons: MouseEventButtons::None,
                    mods: Modifiers::empty(),
                }),
            )
        }
        InputEvent::TextChanged { node_id, value } => {
            DomEvent::new(node_id, DomEventData::Input(BlitzInputEvent { value }))
        }
        InputEvent::FocusGained { node_id } => {
            DomEvent::new(node_id, DomEventData::Focus(BlitzFocusEvent))
        }
        InputEvent::FocusLost { node_id } => {
            DomEvent::new(node_id, DomEventData::Blur(BlitzFocusEvent))
        }
    }
}

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
