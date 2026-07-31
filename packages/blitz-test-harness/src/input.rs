//! Programmatic input synthesis.
//!
//! Free functions construct raw Blitz input events; methods on [`Harness`] synthesize
//! complete interactions (click, tap, drag, typing) and dispatch them through the
//! document's real event pipeline.

use blitz_dom::Document;
use blitz_traits::events::{
    BlitzImeEvent, BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta,
    BlitzWheelEvent, KeyState, MouseEventButton, MouseEventButtons, Point, PointerCoords,
    PointerDetails, UiEvent,
};
use keyboard_types::{Code, Key, Location, Modifiers};
use smol_str::SmolStr;

use crate::Harness;

fn coords(x: f32, y: f32) -> PointerCoords {
    PointerCoords {
        page_x: x,
        page_y: y,
        screen_x: x,
        screen_y: y,
        client_x: x,
        client_y: y,
    }
}

/// Construct a [`BlitzPointerEvent`] at page coordinates `(x, y)`
pub fn pointer_event(
    id: BlitzPointerId,
    x: f32,
    y: f32,
    button: MouseEventButton,
    buttons: MouseEventButtons,
    mods: Modifiers,
) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id,
        is_primary: true,
        coords: coords(x, y),
        button,
        buttons,
        mods,
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    }
}

/// Construct a main-button mouse [`BlitzPointerEvent`] at page coordinates `(x, y)`
pub fn mouse_pointer_event(x: f32, y: f32) -> BlitzPointerEvent {
    pointer_event(
        BlitzPointerId::Mouse,
        x,
        y,
        MouseEventButton::Main,
        MouseEventButtons::from(MouseEventButton::Main),
        Modifiers::default(),
    )
}

/// Construct a finger [`BlitzPointerEvent`] at page coordinates `(x, y)`
pub fn touch_pointer_event(finger: u64, x: f32, y: f32) -> BlitzPointerEvent {
    pointer_event(
        BlitzPointerId::Finger(finger),
        x,
        y,
        MouseEventButton::Main,
        MouseEventButtons::from(MouseEventButton::Main),
        Modifiers::default(),
    )
}

/// Construct a [`BlitzKeyEvent`] for `key` in the given `state`.
///
/// For [`Key::Character`] keys, `text` is populated with the character(s) on press.
pub fn key_event(key: Key, state: KeyState, modifiers: Modifiers) -> BlitzKeyEvent {
    let text = match (&key, state) {
        (Key::Character(text), KeyState::Pressed) => Some(SmolStr::new(text)),
        _ => None,
    };
    BlitzKeyEvent {
        key,
        code: Code::Unidentified,
        modifiers,
        location: Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state,
        text,
    }
}

impl<D: Document> Harness<D> {
    /// Click (pointer down + up) the center of the first element matching `selector`
    pub fn click(&mut self, selector: &str) {
        let (x, y) = self.center_of(selector);
        self.click_at(x, y);
    }

    /// Click (pointer down + up) at page coordinates `(x, y)`
    pub fn click_at(&mut self, x: f32, y: f32) {
        let event = mouse_pointer_event(x, y);
        self.dispatch(UiEvent::PointerDown(event.clone()));
        self.dispatch(UiEvent::PointerUp(event));
        self.pump();
    }

    /// Press the main mouse button at page coordinates `(x, y)`
    pub fn mouse_down_at(&mut self, x: f32, y: f32) {
        self.dispatch(UiEvent::PointerDown(mouse_pointer_event(x, y)));
        self.pump();
    }

    /// Release the main mouse button at page coordinates `(x, y)`
    pub fn mouse_up_at(&mut self, x: f32, y: f32) {
        self.dispatch(UiEvent::PointerUp(mouse_pointer_event(x, y)));
        self.pump();
    }

    /// Move the mouse (no buttons pressed) to page coordinates `(x, y)`
    pub fn move_mouse_to(&mut self, x: f32, y: f32) {
        let event = pointer_event(
            BlitzPointerId::Mouse,
            x,
            y,
            MouseEventButton::Main,
            MouseEventButtons::empty(),
            Modifiers::default(),
        );
        self.dispatch(UiEvent::PointerMove(event));
        self.pump();
    }

    /// Drag the mouse from `(from_x, from_y)` to `(to_x, to_y)` with the main button
    /// pressed, moving in `steps` increments
    pub fn drag(&mut self, from: (f32, f32), to: (f32, f32), steps: u32) {
        let steps = steps.max(1);
        self.dispatch(UiEvent::PointerDown(mouse_pointer_event(from.0, from.1)));
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let x = from.0 + (to.0 - from.0) * t;
            let y = from.1 + (to.1 - from.1) * t;
            self.dispatch(UiEvent::PointerMove(mouse_pointer_event(x, y)));
        }
        self.dispatch(UiEvent::PointerUp(mouse_pointer_event(to.0, to.1)));
        self.pump();
    }

    /// Tap (touch down + up) the center of the first element matching `selector`
    pub fn tap(&mut self, selector: &str) {
        let (x, y) = self.center_of(selector);
        self.tap_at(x, y);
    }

    /// Tap (touch down + up) at page coordinates `(x, y)`
    pub fn tap_at(&mut self, x: f32, y: f32) {
        let event = touch_pointer_event(0, x, y);
        self.dispatch(UiEvent::PointerDown(event.clone()));
        self.dispatch(UiEvent::PointerUp(event));
        self.pump();
    }

    /// Press `finger` down at page coordinates `(x, y)`
    pub fn touch_down(&mut self, finger: u64, x: f32, y: f32) {
        self.dispatch(UiEvent::PointerDown(touch_pointer_event(finger, x, y)));
        self.pump();
    }

    /// Move `finger` to page coordinates `(x, y)`
    pub fn touch_move(&mut self, finger: u64, x: f32, y: f32) {
        self.dispatch(UiEvent::PointerMove(touch_pointer_event(finger, x, y)));
        self.pump();
    }

    /// Lift `finger` at page coordinates `(x, y)`
    pub fn touch_up(&mut self, finger: u64, x: f32, y: f32) {
        self.dispatch(UiEvent::PointerUp(touch_pointer_event(finger, x, y)));
        self.pump();
    }

    /// Scroll with the mouse wheel at page coordinates `(x, y)` by `(delta_x, delta_y)` pixels.
    ///
    /// Negative `delta_y` scrolls the content down (like rolling the wheel towards you).
    /// The mouse is first moved to `(x, y)` so the wheel targets the hovered element.
    pub fn wheel_at(&mut self, x: f32, y: f32, delta_x: f64, delta_y: f64) {
        self.move_mouse_to(x, y);
        let event = BlitzWheelEvent {
            delta: BlitzWheelDelta::Pixels(delta_x, delta_y),
            coords: coords(x, y),
            buttons: MouseEventButtons::empty(),
            mods: Modifiers::default(),
            element: Point::default(),
        };
        self.dispatch(UiEvent::Wheel(event));
        self.pump();
    }

    /// Press and release `key`
    pub fn press(&mut self, key: Key) {
        self.press_with(key, Modifiers::default());
    }

    /// Press and release `key` with `modifiers` held
    pub fn press_with(&mut self, key: Key, modifiers: Modifiers) {
        self.dispatch(UiEvent::KeyDown(key_event(
            key.clone(),
            KeyState::Pressed,
            modifiers,
        )));
        self.dispatch(UiEvent::KeyUp(key_event(
            key,
            KeyState::Released,
            modifiers,
        )));
        self.pump();
    }

    /// Type `text` into the currently focused element, one character at a time
    pub fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.press(Key::Character(ch.to_string()));
        }
    }

    /// Dispatch an IME event
    pub fn ime(&mut self, event: BlitzImeEvent) {
        self.dispatch(UiEvent::Ime(event));
        self.pump();
    }
}
