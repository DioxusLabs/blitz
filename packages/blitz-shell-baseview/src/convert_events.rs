//! Conversions from baseview event types to Blitz event types.
//!
//! baseview uses `keyboard-types` 0.8 while `blitz-traits` uses `keyboard-types` 0.7,
//! so keyboard events are converted between the two versions here.

use blitz_traits::SmolStr;
use blitz_traits::events::{BlitzKeyEvent, KeyState, MouseEventButton};
use cursor_icon::CursorIcon;
use keyboard_types::{Code, Key, Location, Modifiers};

pub(crate) fn kbt08_modifiers_to_kbt07(modifiers: keyboard_types_08::Modifiers) -> Modifiers {
    // Both versions use the same W3C bit layout
    Modifiers::from_bits_truncate(modifiers.bits())
}

fn kbt08_location_to_kbt07(location: keyboard_types_08::Location) -> Location {
    match location {
        keyboard_types_08::Location::Standard => Location::Standard,
        keyboard_types_08::Location::Left => Location::Left,
        keyboard_types_08::Location::Right => Location::Right,
        keyboard_types_08::Location::Numpad => Location::Numpad,
    }
}

fn kbt08_code_to_kbt07(code: keyboard_types_08::Code) -> Code {
    // The `Code` enums are near-identical between versions. Convert via their
    // W3C string representations to avoid a giant variant-by-variant match.
    code.to_string().parse().unwrap_or(Code::Unidentified)
}

fn kbt08_key_to_kbt07(key: &keyboard_types_08::Key) -> Key {
    match key {
        keyboard_types_08::Key::Character(c) => Key::Character(c.to_string()),
        keyboard_types_08::Key::Named(named) => {
            named.to_string().parse().unwrap_or(Key::Unidentified)
        }
    }
}

fn key_text(key: &keyboard_types_08::Key) -> Option<SmolStr> {
    match key {
        keyboard_types_08::Key::Character(c) => Some(SmolStr::new(c)),
        keyboard_types_08::Key::Named(keyboard_types_08::NamedKey::Enter) => {
            Some(SmolStr::new("\r"))
        }
        keyboard_types_08::Key::Named(keyboard_types_08::NamedKey::Tab) => Some(SmolStr::new("\t")),
        keyboard_types_08::Key::Named(_) => None,
    }
}

pub(crate) fn baseview_key_event_to_blitz(
    event: &keyboard_types_08::KeyboardEvent,
) -> BlitzKeyEvent {
    let state = match event.state {
        keyboard_types_08::KeyState::Down => KeyState::Pressed,
        keyboard_types_08::KeyState::Up => KeyState::Released,
    };
    BlitzKeyEvent {
        key: kbt08_key_to_kbt07(&event.key),
        code: kbt08_code_to_kbt07(event.code),
        modifiers: kbt08_modifiers_to_kbt07(event.modifiers),
        location: kbt08_location_to_kbt07(event.location),
        is_auto_repeating: event.repeat,
        is_composing: event.is_composing,
        state,
        text: if state.is_pressed() {
            key_text(&event.key)
        } else {
            None
        },
    }
}

pub(crate) fn baseview_mouse_button_to_blitz(button: baseview::MouseButton) -> MouseEventButton {
    match button {
        baseview::MouseButton::Left => MouseEventButton::Main,
        baseview::MouseButton::Right => MouseEventButton::Secondary,
        baseview::MouseButton::Middle => MouseEventButton::Auxiliary,
        baseview::MouseButton::Back => MouseEventButton::Fourth,
        baseview::MouseButton::Forward => MouseEventButton::Fifth,
        _ => MouseEventButton::Auxiliary,
    }
}

/// Convert a Blitz [`CursorIcon`] to a baseview [`MouseCursor`](baseview::MouseCursor).
/// `None` means the cursor should be hidden.
pub(crate) fn cursor_icon_to_baseview(icon: Option<CursorIcon>) -> baseview::MouseCursor {
    use baseview::MouseCursor;
    let Some(icon) = icon else {
        return MouseCursor::Hidden;
    };
    match icon {
        CursorIcon::Default => MouseCursor::Default,
        CursorIcon::Pointer => MouseCursor::Hand,
        CursorIcon::Grab => MouseCursor::Hand,
        CursorIcon::Grabbing => MouseCursor::HandGrabbing,
        CursorIcon::Help => MouseCursor::Help,
        CursorIcon::Text => MouseCursor::Text,
        CursorIcon::VerticalText => MouseCursor::VerticalText,
        CursorIcon::Wait => MouseCursor::Working,
        CursorIcon::Progress => MouseCursor::PtrWorking,
        CursorIcon::NotAllowed => MouseCursor::NotAllowed,
        CursorIcon::NoDrop => MouseCursor::PtrNotAllowed,
        CursorIcon::ZoomIn => MouseCursor::ZoomIn,
        CursorIcon::ZoomOut => MouseCursor::ZoomOut,
        CursorIcon::Alias => MouseCursor::Alias,
        CursorIcon::Copy => MouseCursor::Copy,
        CursorIcon::Move => MouseCursor::Move,
        CursorIcon::AllScroll => MouseCursor::AllScroll,
        CursorIcon::Cell => MouseCursor::Cell,
        CursorIcon::Crosshair => MouseCursor::Crosshair,
        CursorIcon::EResize => MouseCursor::EResize,
        CursorIcon::NResize => MouseCursor::NResize,
        CursorIcon::NeResize => MouseCursor::NeResize,
        CursorIcon::NwResize => MouseCursor::NwResize,
        CursorIcon::SResize => MouseCursor::SResize,
        CursorIcon::SeResize => MouseCursor::SeResize,
        CursorIcon::SwResize => MouseCursor::SwResize,
        CursorIcon::WResize => MouseCursor::WResize,
        CursorIcon::EwResize => MouseCursor::EwResize,
        CursorIcon::NsResize => MouseCursor::NsResize,
        CursorIcon::NwseResize => MouseCursor::NwseResize,
        CursorIcon::NeswResize => MouseCursor::NeswResize,
        CursorIcon::ColResize => MouseCursor::ColResize,
        CursorIcon::RowResize => MouseCursor::RowResize,
        _ => MouseCursor::Default,
    }
}
