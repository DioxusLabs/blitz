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
    pub current_target: Option<usize>,
    composed_path: Vec<usize>,
    /// Where true indicates that the default user agent action was prevented,
    /// and false indicates that it was not.
    pub default_prevented: bool,

    pub stop_propagation: bool,
    pub data: DomEventData,
}

impl DomEvent {
    pub fn new(target: usize, data: DomEventData, composed_path: Vec<usize>) -> Self {
        let mut cancelable = true;
        let mut bubbles = true;

        match data.name() {
            "input" => {
                cancelable = false;
            }
            "focus" | "blur" => {
                cancelable = false;
                bubbles = false;
            }
            _ => {}
        }

        Self {
            target,
            bubbles,
            cancelable,
            current_target: None,
            composed_path,
            default_prevented: false,

            stop_propagation: false,
            data,
        }
    }

    pub fn composed_path(&self) -> &Vec<usize> {
        &self.composed_path
    }

    pub fn prevent_default(&mut self) {
        if !self.cancelable {
            return;
        }
        self.default_prevented = true;
    }

    pub fn stop_propagation(&mut self) {
        self.stop_propagation = true;
    }

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
    Focus,
    Blur,
    Input(BlitzKeyEvent),
    KeyDown(BlitzKeyEvent),
    KeyUp(BlitzKeyEvent),
    KeyPress(BlitzKeyEvent),
    Ime(BlitzImeEvent),
    Hover,
    /// A string containing the type of Event.
    Event(&'static str),
}

impl DomEventData {
    pub fn name(&self) -> &'static str {
        match self {
            DomEventData::MouseDown { .. } => "mousedown",
            DomEventData::MouseUp { .. } => "mouseup",
            DomEventData::Click { .. } => "click",
            DomEventData::Focus => "focus",
            DomEventData::Blur => "blur",
            DomEventData::Input { .. } => "input",
            DomEventData::KeyDown { .. } => "keydown",
            DomEventData::KeyUp { .. } => "keyup",
            DomEventData::KeyPress { .. } => "keypress",
            DomEventData::Ime { .. } => "input",
            DomEventData::Hover => "mouseover",
            DomEventData::Event(event_type) => event_type,
        }
    }
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
