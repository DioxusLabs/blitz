use keyboard_types::{Code, Key, Location, Modifiers};
use smol_str::SmolStr;

pub struct EventListener {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct DomEvent {
    pub target: usize,
    pub data: DomEventData,
}

impl DomEvent {
    /// Returns the name of the event ("click", "mouseover", "keypress", etc)
    pub fn name(&self) -> &'static str {
        self.data.name()
    }
}

#[derive(Debug, Clone)]
pub enum DomEventData {
    MouseDown(BlitzMouseButtonEvent),
    MouseUp(BlitzMouseButtonEvent),
    Click(BlitzMouseButtonEvent),
    KeyPress(BlitzKeyEvent),
    Ime(BlitzImeEvent),
    Hover,
}

impl DomEventData {
    pub fn name(&self) -> &'static str {
        match self {
            DomEventData::MouseDown { .. } => "mousedown",
            DomEventData::MouseUp { .. } => "mouseup",
            DomEventData::Click { .. } => "click",
            DomEventData::KeyPress { .. } => "keypress",
            DomEventData::Ime { .. } => "input",
            DomEventData::Hover => "mouseover",
        }
    }
}

#[derive(Debug, Clone)]
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
    pub mods: Modifiers,
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
