use kurbo::Point;

/// Keyboard modifier state
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// The Command key on macOS, the Windows/Super key elsewhere
    pub meta: bool,
}

/// The keys the editor has bindings for. Anything else is [`Key::Other`], and may still
/// insert text through [`KeyInput::text`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// A printable key, as the character(s) it produces without modifiers applied
    Character(String),
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    Other,
}

/// A key press
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyInput {
    pub key: Key,
    pub mods: Modifiers,
    /// The text this key press would insert, if any
    pub text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

/// Input method editor events
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeInput {
    Enabled,
    /// Composition text, with the IME's cursor as a byte range within it
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    Commit(String),
    DeleteSurrounding {
        before_bytes: usize,
        after_bytes: usize,
    },
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollDelta {
    /// Scroll by lines (positive y scrolls up)
    Lines(f64, f64),
    /// Scroll by logical pixels (positive y scrolls up)
    Pixels(f64, f64),
}

/// Input for a [`CodeEditor`](crate::CodeEditor). Positions are logical pixels relative to the
/// editor's top-left corner.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorInput {
    Key(KeyInput),
    Ime(ImeInput),
    PointerDown {
        pos: Point,
        button: PointerButton,
        mods: Modifiers,
        /// 1 for a single click, 2 for a double click, 3 for a triple click
        click_count: u8,
    },
    PointerMove {
        pos: Point,
    },
    PointerUp {
        pos: Point,
        button: PointerButton,
    },
    Wheel {
        delta: ScrollDelta,
    },
    /// Clipboard text, sent in reply to [`EditorResponse::request_clipboard`](crate::EditorResponse::request_clipboard)
    Paste(String),
    /// The editor gained or lost focus
    Focus(bool),
    /// The delay from [`EditorResponse::timer`](crate::EditorResponse::timer) elapsed
    Timer,
}
