//! Types to represent UI and DOM events

use std::str::FromStr;

use bitflags::bitflags;
use keyboard_types::{Code, Key, Location, Modifiers};
use smol_str::SmolStr;

#[derive(Default)]
pub struct EventState {
    cancelled: bool,
    propagation_stopped: bool,
    redraw_requested: bool,
}
impl EventState {
    #[inline(always)]
    pub fn prevent_default(&mut self) {
        self.cancelled = true;
    }

    #[inline(always)]
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    #[inline(always)]
    pub fn request_redraw(&mut self) {
        self.redraw_requested = true;
    }

    #[inline(always)]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    #[inline(always)]
    pub fn propagation_is_stopped(&self) -> bool {
        self.propagation_stopped
    }

    #[inline(always)]
    pub fn redraw_is_requested(&self) -> bool {
        self.redraw_requested
    }
}

#[derive(Debug, Clone)]
#[repr(u8)]
pub enum UiEvent {
    MouseMove(BlitzMouseButtonEvent),
    MouseUp(BlitzMouseButtonEvent),
    MouseDown(BlitzMouseButtonEvent),
    KeyUp(BlitzKeyEvent),
    KeyDown(BlitzKeyEvent),
    Ime(BlitzImeEvent),
}
impl UiEvent {
    pub fn discriminant(&self) -> u8 {
        // SAFETY: Because `Self` is marked `repr(u8)`, its layout is a `repr(C)` `union`
        // between `repr(C)` structs, each of which has the `u8` discriminant as its first
        // field, so we can read the discriminant without offsetting the pointer.
        // See: https://doc.rust-lang.org/stable/std/mem/fn.discriminant.html#accessing-the-numeric-value-of-the-discriminant
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
}

#[derive(Debug, Clone)]
pub struct DomEvent {
    pub target: usize,
    /// Which is true if the event bubbles up through the DOM tree.
    pub bubbles: bool,
    /// which is true if the event can be canceled.
    pub cancelable: bool,

    pub data: DomEventData,
    pub request_redraw: bool,
}

impl DomEvent {
    pub fn new(target: usize, data: DomEventData) -> Self {
        Self {
            target,
            bubbles: data.bubbles(),
            cancelable: data.cancelable(),
            data,
            request_redraw: false,
        }
    }

    /// Returns the name of the event ("click", "mouseover", "keypress", etc)
    pub fn name(&self) -> &'static str {
        self.data.name()
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum DomEventKind {
    MouseMove,
    MouseDown,
    MouseUp,
    Click,
    KeyPress,
    KeyDown,
    KeyUp,
    Input,
    Ime,
}
impl DomEventKind {
    pub fn discriminant(self) -> u8 {
        self as u8
    }
}
impl FromStr for DomEventKind {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s.trim_start_matches("on") {
            "mousemove" => Ok(Self::MouseMove),
            "mousedown" => Ok(Self::MouseDown),
            "mouseup" => Ok(Self::MouseUp),
            "click" => Ok(Self::Click),
            "keypress" => Ok(Self::KeyPress),
            "keydown" => Ok(Self::KeyDown),
            "keyup" => Ok(Self::KeyUp),
            "input" => Ok(Self::Input),
            "composition" => Ok(Self::Ime),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
#[repr(u8)]
pub enum DomEventData {
    MouseMove(BlitzMouseButtonEvent),
    MouseDown(BlitzMouseButtonEvent),
    MouseUp(BlitzMouseButtonEvent),
    Click(BlitzMouseButtonEvent),
    KeyPress(BlitzKeyEvent),
    KeyDown(BlitzKeyEvent),
    KeyUp(BlitzKeyEvent),
    Input(BlitzInputEvent),
    Ime(BlitzImeEvent),
    MouseOver(BlitzMousePositionEvent),
    MouseEnter(BlitzMousePositionEvent),
    MouseOut(BlitzMousePositionEvent),
}
impl DomEventData {
    pub fn discriminant(&self) -> u8 {
        // SAFETY: Because `Self` is marked `repr(u8)`, its layout is a `repr(C)` `union`
        // between `repr(C)` structs, each of which has the `u8` discriminant as its first
        // field, so we can read the discriminant without offsetting the pointer.
        // See: https://doc.rust-lang.org/stable/std/mem/fn.discriminant.html#accessing-the-numeric-value-of-the-discriminant
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
}

impl DomEventData {
    pub fn name(&self) -> &'static str {
        match self {
            Self::MouseMove { .. } => "mousemove",
            Self::MouseDown { .. } => "mousedown",
            Self::MouseUp { .. } => "mouseup",
            Self::Click { .. } => "click",
            Self::KeyPress { .. } => "keypress",
            Self::KeyDown { .. } => "keydown",
            Self::KeyUp { .. } => "keyup",
            Self::Input { .. } => "input",
            Self::Ime { .. } => "composition",
            Self::MouseOver { .. } => "mouseover",
            Self::MouseEnter { .. } => "mouseenter",
            Self::MouseOut { .. } => "mouseout",
            Self::MouseLeave { .. } => "mouseleave",
        }
    }

    pub fn kind(&self) -> DomEventKind {
        match self {
            Self::MouseMove { .. } => DomEventKind::MouseMove,
            Self::MouseDown { .. } => DomEventKind::MouseDown,
            Self::MouseUp { .. } => DomEventKind::MouseUp,
            Self::Click { .. } => DomEventKind::Click,
            Self::KeyPress { .. } => DomEventKind::KeyPress,
            Self::KeyDown { .. } => DomEventKind::KeyDown,
            Self::KeyUp { .. } => DomEventKind::KeyUp,
            Self::Input { .. } => DomEventKind::Input,
            Self::Ime { .. } => DomEventKind::Ime,
            Self::MouseOver { .. } => DomEventKind::MouseMove, // No specific enum for these
            Self::MouseEnter { .. } => DomEventKind::MouseMove,
            Self::MouseOut { .. } => DomEventKind::MouseMove,
            Self::MouseLeave { .. } => DomEventKind::MouseMove,
        }
    }

    pub fn cancelable(&self) -> bool {
        match self {
            Self::MouseMove { .. } => true,
            Self::MouseDown { .. } => true,
            Self::MouseUp { .. } => true,
            Self::Click { .. } => true,
            Self::KeyDown { .. } => true,
            Self::KeyUp { .. } => true,
            Self::KeyPress { .. } => true,
            Self::Ime { .. } => true,
            Self::Input { .. } => false,
            Self::MouseOver { .. } => true,
            Self::MouseEnter { .. } => false,
            Self::MouseOut { .. } => true,
            Self::MouseLeave { .. } => false,
        }
    }

    pub fn bubbles(&self) -> bool {
        match self {
            Self::MouseMove { .. } => true,
            Self::MouseDown { .. } => true,
            Self::MouseUp { .. } => true,
            Self::Click { .. } => true,
            Self::KeyDown { .. } => true,
            Self::KeyUp { .. } => true,
            Self::KeyPress { .. } => true,
            Self::Ime { .. } => true,
            Self::Input { .. } => true,
            Self::MouseOver { .. } => true,
            Self::MouseEnter { .. } => false,
            Self::MouseOut { .. } => true,
            Self::MouseLeave { .. } => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BlitzMousePositionEvent {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct HitResult {
    /// The node_id of the node identified as the hit target
    pub node_id: usize,
    /// The x coordinate of the hit within the hit target's border-box
    pub x: f32,
    /// The y coordinate of the hit within the hit target's border-box
    pub y: f32,
}

#[derive(Clone, Debug)]
pub struct BlitzMouseButtonEvent {
    pub x: f32,
    pub y: f32,
    pub button: MouseEventButton,
    pub buttons: MouseEventButtons,
    pub mods: Modifiers,
}

bitflags! {
    /// The buttons property indicates which buttons are pressed on the mouse
    /// (or other input device) when a mouse event is triggered.
    ///
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/MouseEvent/buttons)
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct MouseEventButtons: u8 {
        /// 0: No button or un-initialized
        const None = 0b0000_0000;
        /// 1: Primary button (usually the left button)
        const Primary = 0b0000_0001;
        /// 2: Secondary button (usually the right button)
        const Secondary = 0b0000_0010;
        /// 4: Auxiliary button (usually the mouse wheel button or middle button)
        const Auxiliary = 0b0000_0100;
        /// 8: 4th button (typically the "Browser Back" button)
        const Fourth = 0b0000_1000;
        /// 16: 5th button (typically the "Browser Forward" button)
        const Fifth = 0b0001_0000;
    }
}

impl Default for MouseEventButtons {
    fn default() -> Self {
        Self::None
    }
}

impl From<MouseEventButton> for MouseEventButtons {
    fn from(value: MouseEventButton) -> Self {
        match value {
            MouseEventButton::Main => Self::Primary,
            MouseEventButton::Auxiliary => Self::Auxiliary,
            MouseEventButton::Secondary => Self::Secondary,
            MouseEventButton::Fourth => Self::Fourth,
            MouseEventButton::Fifth => Self::Fifth,
        }
    }
}

/// The button property indicates which button was pressed
/// on the mouse to trigger the event.
///
/// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/MouseEvent/button)
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum MouseEventButton {
    /// Main button pressed, usually the left button or the un-initialized state
    #[default]
    Main = 0,
    /// Auxiliary button pressed, usually the wheel button or the middle button (if present)
    Auxiliary = 1,
    /// Secondary button pressed, usually the right button
    Secondary = 2,
    /// Fourth button, typically the Browser Back button
    Fourth = 3,
    /// Fifth button, typically the Browser Forward button
    Fifth = 4,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

impl KeyState {
    pub fn is_pressed(self) -> bool {
        matches!(self, Self::Pressed)
    }
}

#[derive(Clone, Debug)]
pub struct BlitzKeyEvent {
    pub key: Key,
    pub code: Code,
    pub modifiers: Modifiers,
    pub location: Location,
    pub is_auto_repeating: bool,
    pub is_composing: bool,
    pub state: KeyState,
    pub text: Option<SmolStr>,
}

#[derive(Clone, Debug)]
pub struct BlitzInputEvent {
    pub value: String,
}

/// Copy of Winit IME event to avoid lower-level Blitz crates depending on winit
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BlitzImeEvent {
    /// Notifies when the IME was enabled.
    ///
    /// After getting this event you could receive [`Preedit`][Self::Preedit] and
    /// [`Commit`][Self::Commit] events.
    Enabled,

    /// Notifies when a new composing text should be set at the cursor position.
    ///
    /// The value represents a pair of the preedit string and the cursor begin position and end
    /// position. When it's `None`, the cursor should be hidden. When `String` is an empty string
    /// this indicates that preedit was cleared.
    ///
    /// The cursor position is byte-wise indexed.
    Preedit(String, Option<(usize, usize)>),

    /// Notifies when text should be inserted into the editor widget.
    ///
    /// Right before this event winit will send empty [`Self::Preedit`] event.
    Commit(String),

    /// Notifies when the IME was disabled.
    ///
    /// After receiving this event you won't get any more [`Preedit`][Self::Preedit] or
    /// [`Commit`][Self::Commit] events until the next [`Enabled`][Self::Enabled] event.
    Disabled,
}
