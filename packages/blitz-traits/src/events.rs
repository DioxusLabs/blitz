use bitflags::bitflags;
use keyboard_types::{Code, Key, Location, Modifiers};
use smol_str::SmolStr;
pub struct EventListener {
    pub name: String,
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

#[derive(Debug, Clone)]
pub enum DomEventData {
    MouseMove(BlitzMouseButtonEvent),
    MouseDown(BlitzMouseButtonEvent),
    MouseUp(BlitzMouseButtonEvent),
    Click(BlitzMouseButtonEvent),
    KeyPress(BlitzKeyEvent),
    Ime(BlitzImeEvent),
    MouseOver(BlitzMouseOverEvent),
    MouseLeave,
}

impl DomEventData {
    pub fn name(&self) -> &'static str {
        match self {
            Self::MouseMove { .. } => "mousemove",
            Self::MouseDown { .. } => "mousedown",
            Self::MouseUp { .. } => "mouseup",
            Self::Click { .. } => "click",
            Self::KeyPress { .. } => "keypress",
            Self::Ime { .. } => "input",
            Self::MouseOver { .. } => "mouseover",
            Self::MouseLeave => "mouseleave",
        }
    }

    pub fn cancelable(&self) -> bool {
        match self {
            Self::MouseMove { .. } => true,
            Self::MouseDown { .. } => true,
            Self::MouseUp { .. } => true,
            Self::Click { .. } => true,
            Self::KeyPress { .. } => true,
            Self::Ime { .. } => true,
            Self::MouseOver { .. } => true,
            Self::MouseLeave => false,
        }
    }

    pub fn bubbles(&self) -> bool {
        match self {
            Self::MouseMove { .. } => true,
            Self::MouseDown { .. } => true,
            Self::MouseUp { .. } => true,
            Self::Click { .. } => true,
            Self::KeyPress { .. } => true,
            Self::Ime { .. } => true,
            Self::MouseOver { .. } => true,
            Self::MouseLeave => true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BlitzMouseOverEvent {
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
    /// The cursor position is byte-wise indexed.
    Preedit(String, Option<(usize, usize)>),

    /// Notifies when text should be inserted into the editor widget.
    ///
    /// Right before this event winit will send empty [`Self::Preedit`] event.
    Commit(String),

    /// Notifies when the IME was disabled.
    ///
    /// After receiving this event you won't get any more [`Preedit`][Self::Preedit] or
    /// [`Commit`][Self::Commit] events until the next [`Enabled`][Self::Enabled] event. You should
    /// also stop issuing IME related requests like [`Window::set_ime_cursor_area`] and clear
    /// pending preedit text.
    Disabled,
}
