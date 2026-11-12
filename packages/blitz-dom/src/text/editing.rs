//! Text editing for inputs and textareas.

use std::borrow::Cow;
use std::ops::Range;

use blitz_traits::node_id::NodeId;
use kurbo::{Rect, Size};

use super::{TextContext, backend};

/// The text of an `<input>` or `<textarea>`, as the text backend edits and lays it out.
pub type TextEditor = backend::TextEditor;

/// The size and scale of the laid-out text of an `<input>` or `<textarea>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorMetrics {
    /// How wide and tall the laid-out text is, in device pixels.
    pub size: Size,
    /// Device pixels per CSS pixel that the text is laid out at.
    pub scale: f32,
}

/// The text of an `<input>` or `<textarea>`, as it is read back.
pub trait EditableText: Clone {
    /// The text, with any text an input method is composing, which the selection indexes.
    fn raw_text(&self) -> &str;

    /// The text without any text an input method is composing, as a form submits it: borrowed
    /// where nothing is composing.
    fn text(&self) -> Cow<'_, str>;

    /// The selection's byte range in [`raw_text`](Self::raw_text).
    fn selection(&self) -> Range<usize>;

    /// The selected text, where the selection is not empty.
    fn selected_text(&self) -> Option<&str>;

    /// Whether the selection is a caret.
    fn is_selection_collapsed(&self) -> bool;

    /// The size and scale of the laid-out text, or `None` where it is not laid out.
    fn metrics(&self) -> Option<EditorMetrics>;

    /// The caret, in device pixels relative to the text's origin, where the editor shows one.
    fn caret_rect(&self) -> Option<Rect>;
}

/// Changing the text of an `<input>` or `<textarea>`: the part of [`EditableText`] the document
/// drives.
///
/// The setters record what changes, and the text is laid out again once, by the next
/// [`refresh`](Self::refresh) or [`edit`](Self::edit).
pub(crate) trait EditEngine: EditableText + Sized {
    /// An empty editor.
    fn new(is_multiline: bool) -> Self;

    /// Sets the text the editor starts with.
    fn set_initial_text(&mut self, text: &str);

    /// Sets the text, where it changed.
    fn set_text(&mut self, text: &str);

    /// Sets the text in `style`, `node`'s computed style, at `scale`, with lines unwrapped until a
    /// width is set.
    fn set_style(
        &mut self,
        node: NodeId,
        style: Option<&style::properties::ComputedValues>,
        scale: f32,
    );

    /// Sets the scale.
    fn set_scale(&mut self, scale: f32);

    /// Sets the width lines wrap at, in device pixels.
    fn set_width(&mut self, width: Option<f32>);

    /// Lays the text out again, where a setter changed it since it was last laid out.
    fn refresh(&mut self, cx: &mut TextContext);

    /// Applies `edit`, leaving the text laid out.
    fn edit(&mut self, cx: &mut TextContext, edit: Edit<'_>);
}

/// A change to the text or the selection of an `<input>` or `<textarea>`.
///
/// Points are in device pixels relative to the text's origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit<'a> {
    /// Replaces the selection with the text.
    Insert(&'a str),
    /// Deletes the selection, or the character after the caret.
    Delete,
    /// Deletes the selection, or the character before the caret.
    Backdelete,
    /// Deletes the selection, or the word after the caret.
    DeleteWord,
    /// Deletes the selection, or the word before the caret.
    BackdeleteWord,
    /// Deletes the selection.
    DeleteSelection,
    /// Moves the caret, collapsing the selection.
    Move(Motion),
    /// Moves the selection's focus, keeping its anchor.
    Extend(Motion),
    /// Selects all the text.
    SelectAll,
    /// Collapses the selection to its focus.
    CollapseSelection,
    /// Selects the bytes from the first offset to the second.
    SelectByteRange(usize, usize),
    /// Moves the caret by the first motion, then extends the selection by the second:
    /// `Select(LineStart, LineEnd)` selects the line.
    Select(Motion, Motion),
    /// Sets the text an input method is composing, and the cursor within it.
    SetCompose(&'a str, Option<(usize, usize)>),
    /// Clears the text an input method is composing.
    ClearCompose,
    /// Moves the caret to the point.
    MoveToPoint(f32, f32),
    /// Extends the selection to the point, as a drag does.
    ExtendSelectionToPoint(f32, f32),
    /// Extends the selection to the point, as a shift-click does.
    ShiftClickExtension(f32, f32),
    /// Selects the word at the point.
    SelectWordAtPoint(f32, f32),
    /// Selects the hard line at the point.
    SelectHardLineAtPoint(f32, f32),
}

/// Where an [`Edit::Move`] or an [`Edit::Extend`] takes the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// One character to the left.
    Left,
    /// One character to the right.
    Right,
    /// One word to the left.
    WordLeft,
    /// One word to the right.
    WordRight,
    /// One line up.
    Up,
    /// One line down.
    Down,
    /// The start of the visual line.
    LineStart,
    /// The end of the visual line.
    LineEnd,
    /// The start of the line, as the text's line breaks end it.
    HardLineStart,
    /// The end of the line, as the text's line breaks end it.
    HardLineEnd,
    /// The start of the text.
    TextStart,
    /// The end of the text.
    TextEnd,
}

/// An `<input>` or `<textarea>`'s editor with the document's text context, handed to
/// [`BaseDocument::with_text_input`](crate::BaseDocument::with_text_input).
pub struct TextInputDriver<'a> {
    pub(crate) editor: &'a mut TextEditor,
    pub(crate) cx: &'a mut TextContext,
}

impl TextInputDriver<'_> {
    /// The editor, to read back.
    #[inline]
    pub fn editor(&self) -> &TextEditor {
        self.editor
    }

    /// The text, with any text an input method is composing.
    #[inline]
    pub fn raw_text(&self) -> &str {
        self.editor.raw_text()
    }

    /// Applies `edit`.
    #[inline]
    pub fn edit(&mut self, edit: Edit<'_>) {
        self.editor.edit(self.cx, edit);
    }

    /// Selects all the text.
    pub fn select_all(&mut self) {
        self.edit(Edit::SelectAll);
    }

    /// Selects the bytes from `start` to `end`.
    pub fn select_byte_range(&mut self, start: usize, end: usize) {
        self.edit(Edit::SelectByteRange(start, end));
    }
}
