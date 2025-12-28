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
    MouseMove(BlitzPointerEvent),
    MouseUp(BlitzPointerEvent),
    MouseDown(BlitzPointerEvent),
    Wheel(BlitzWheelEvent),
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
    MouseEnter,
    MouseLeave,
    MouseOver,
    MouseOut,
    Scroll,
    Wheel,
    Click,
    ContextMenu,
    DoubleClick,
    KeyPress,
    KeyDown,
    KeyUp,
    Input,
    Ime,
    Focus,
    Blur,
    FocusIn,
    FocusOut,
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
            "mouseenter" => Ok(Self::MouseEnter),
            "mouseleave" => Ok(Self::MouseLeave),
            "mouseover" => Ok(Self::MouseOver),
            "mouseout" => Ok(Self::MouseOut),
            "scroll" => Ok(Self::Scroll),
            "wheel" => Ok(Self::Wheel),
            "click" => Ok(Self::Click),
            "contextmenu" => Ok(Self::ContextMenu),
            "dblclick" => Ok(Self::DoubleClick),
            "keypress" => Ok(Self::KeyPress),
            "keydown" => Ok(Self::KeyDown),
            "keyup" => Ok(Self::KeyUp),
            "input" => Ok(Self::Input),
            "composition" => Ok(Self::Ime),
            "focus" => Ok(Self::Focus),
            "blur" => Ok(Self::Blur),
            "focusin" => Ok(Self::FocusIn),
            "focusout" => Ok(Self::FocusOut),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
#[repr(u8)]
pub enum DomEventData {
    MouseMove(BlitzPointerEvent),
    MouseDown(BlitzPointerEvent),
    MouseUp(BlitzPointerEvent),
    MouseEnter(BlitzPointerEvent),
    MouseLeave(BlitzPointerEvent),
    MouseOver(BlitzPointerEvent),
    MouseOut(BlitzPointerEvent),
    Scroll(BlitzScrollEvent),
    Wheel(BlitzWheelEvent),
    Click(BlitzPointerEvent),
    ContextMenu(BlitzPointerEvent),
    DoubleClick(BlitzPointerEvent),
    KeyPress(BlitzKeyEvent),
    KeyDown(BlitzKeyEvent),
    KeyUp(BlitzKeyEvent),
    Input(BlitzInputEvent),
    Ime(BlitzImeEvent),
    Focus(BlitzFocusEvent),
    Blur(BlitzFocusEvent),
    FocusIn(BlitzFocusEvent),
    FocusOut(BlitzFocusEvent),
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
            Self::MouseEnter { .. } => "mouseenter",
            Self::MouseLeave { .. } => "mouseleave",
            Self::MouseOver { .. } => "mouseover",
            Self::MouseOut { .. } => "mouseout",
            Self::Scroll { .. } => "scroll",
            Self::Wheel { .. } => "wheel",
            Self::Click { .. } => "click",
            Self::ContextMenu { .. } => "contextmenu",
            Self::DoubleClick { .. } => "dblclick",
            Self::KeyPress { .. } => "keypress",
            Self::KeyDown { .. } => "keydown",
            Self::KeyUp { .. } => "keyup",
            Self::Input { .. } => "input",
            Self::Ime { .. } => "composition",
            Self::Focus { .. } => "focus",
            Self::Blur { .. } => "blur",
            Self::FocusIn { .. } => "focusin",
            Self::FocusOut { .. } => "focusout",
        }
    }

    pub fn kind(&self) -> DomEventKind {
        match self {
            Self::MouseMove { .. } => DomEventKind::MouseMove,
            Self::MouseDown { .. } => DomEventKind::MouseDown,
            Self::MouseUp { .. } => DomEventKind::MouseUp,
            Self::MouseEnter { .. } => DomEventKind::MouseEnter,
            Self::MouseLeave { .. } => DomEventKind::MouseLeave,
            Self::MouseOver { .. } => DomEventKind::MouseOver,
            Self::MouseOut { .. } => DomEventKind::MouseOut,
            Self::Scroll { .. } => DomEventKind::Scroll,
            Self::Wheel { .. } => DomEventKind::Wheel,
            Self::Click { .. } => DomEventKind::Click,
            Self::ContextMenu { .. } => DomEventKind::ContextMenu,
            Self::DoubleClick { .. } => DomEventKind::DoubleClick,
            Self::KeyPress { .. } => DomEventKind::KeyPress,
            Self::KeyDown { .. } => DomEventKind::KeyDown,
            Self::KeyUp { .. } => DomEventKind::KeyUp,
            Self::Input { .. } => DomEventKind::Input,
            Self::Ime { .. } => DomEventKind::Ime,
            Self::Focus { .. } => DomEventKind::Focus,
            Self::Blur { .. } => DomEventKind::Blur,
            Self::FocusIn { .. } => DomEventKind::FocusIn,
            Self::FocusOut { .. } => DomEventKind::FocusOut,
        }
    }

    pub fn cancelable(&self) -> bool {
        match self {
            Self::MouseMove { .. } => true,
            Self::MouseDown { .. } => true,
            Self::MouseUp { .. } => true,
            Self::MouseEnter { .. } => false,
            Self::MouseLeave { .. } => false,
            Self::MouseOver { .. } => true,
            Self::MouseOut { .. } => true,
            Self::Scroll { .. } => false,
            Self::Wheel { .. } => true,
            Self::Click { .. } => true,
            Self::ContextMenu { .. } => true,
            Self::DoubleClick { .. } => true,
            Self::KeyDown { .. } => true,
            Self::KeyUp { .. } => true,
            Self::KeyPress { .. } => true,
            Self::Ime { .. } => true,
            Self::Input { .. } => false,
            Self::Focus { .. } => false,
            Self::Blur { .. } => false,
            Self::FocusIn { .. } => false,
            Self::FocusOut { .. } => false,
        }
    }

    pub fn bubbles(&self) -> bool {
        match self {
            Self::MouseMove { .. } => true,
            Self::MouseDown { .. } => true,
            Self::MouseUp { .. } => true,
            Self::MouseEnter { .. } => false,
            Self::MouseLeave { .. } => false,
            Self::MouseOver { .. } => true,
            Self::MouseOut { .. } => true,
            Self::Scroll { .. } => false,
            Self::Wheel { .. } => true,
            Self::Click { .. } => true,
            Self::ContextMenu { .. } => true,
            Self::DoubleClick { .. } => true,
            Self::KeyDown { .. } => true,
            Self::KeyUp { .. } => true,
            Self::KeyPress { .. } => true,
            Self::Ime { .. } => true,
            Self::Input { .. } => true,
            Self::Focus { .. } => false,
            Self::Blur { .. } => false,
            Self::FocusIn { .. } => true,
            Self::FocusOut { .. } => true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HitResult {
    /// The node_id of the node identified as the hit target
    pub node_id: usize,
    /// Whether the hit content is text
    pub is_text: bool,
    /// The x coordinate of the hit within the hit target's border-box
    pub x: f32,
    /// The y coordinate of the hit within the hit target's border-box
    pub y: f32,
}

#[derive(Clone, Debug)]
pub struct BlitzPointerEvent {
    pub x: f32,
    pub y: f32,
    pub button: MouseEventButton,
    pub buttons: MouseEventButtons,
    pub mods: Modifiers,
}

#[derive(Clone, Debug)]
pub struct BlitzWheelEvent {
    pub delta: BlitzWheelDelta,
    pub x: f32,
    pub y: f32,
    pub button: MouseEventButton,
    pub buttons: MouseEventButtons,
    pub mods: Modifiers,
}

#[derive(Clone, Debug)]
pub enum BlitzWheelDelta {
    Lines(f64, f64),
    Pixels(f64, f64),
}

#[derive(Clone, Debug)]
pub struct BlitzScrollEvent {
    pub scroll_top: f64,
    pub scroll_left: f64,
    pub scroll_width: i32,
    pub scroll_height: i32,
    pub client_width: i32,
    pub client_height: i32,
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

#[derive(Clone, Debug)]
pub struct BlitzFocusEvent;

/// Copy of Winit IME event to avoid lower-level Blitz crates depending on winit
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BlitzImeEvent {
    /// Notifies when the IME was enabled.
    ///
    /// After getting this event you could receive [`Preedit`][Self::Preedit] and
    /// [`Commit`][Self::Commit] events. You should also start performing IME related requests
    /// like [`Window::set_ime_cursor_area`].
    Enabled,

    /// Notifies when a new composing text should be set at the cursor position.
    ///
    /// The value represents a pair of the preedit string and the cursor begin position and end
    /// position. When it's `None`, the cursor should be hidden. When `String` is an empty string
    /// this indicates that preedit was cleared.
    ///
    /// The cursor position is byte-wise indexed, assuming UTF-8.
    Preedit(String, Option<(usize, usize)>),

    /// Notifies when text should be inserted into the editor widget.
    ///
    /// Right before this event winit will send empty [`Self::Preedit`] event.
    Commit(String),

    /// Delete text surrounding the cursor or selection.
    ///
    /// This event does not affect either the pre-edit string.
    /// This means that the application must first remove the pre-edit,
    /// then execute the deletion, then insert the removed text back.
    ///
    /// This event assumes text is stored in UTF-8.
    DeleteSurrounding {
        /// Bytes to remove before the selection
        before_bytes: usize,
        /// Bytes to remove after the selection
        after_bytes: usize,
    },

    /// Notifies when the IME was disabled.
    ///
    /// After receiving this event you won't get any more [`Preedit`][Self::Preedit] or
    /// [`Commit`][Self::Commit] events until the next [`Enabled`][Self::Enabled] event. You should
    /// also stop issuing IME related requests like [`Window::set_ime_cursor_area`] and clear
    /// pending preedit text.
    Disabled,
}
