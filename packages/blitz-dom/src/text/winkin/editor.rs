//! The text of an `<input>` or `<textarea>` under winkin.
//!
//! The value is laid out by winkin as plain text in the control's style, on one line or wrapped
//! at the control's width, with white space preserved. Text is inserted and deleted at the
//! selection, which winkin's selection motions move by character, word, line, paragraph and the
//! whole text, and points are hit tested against the lines. Text an input method is composing is
//! laid out in place of the selection, underlined, until it is committed or cancelled. Every
//! change to the text can be undone and redone.

use std::borrow::Cow;
use std::ops::Range;

use blitz_traits::node_id::NodeId;
use kurbo::{Rect, Size};
use style::properties::ComputedValues;
use style::servo_arc::Arc as ServoArc;
use winkin::config::PastLines;
use winkin::selection::{Granularity, MotionDirection, Position, Selection};

use super::{TextContext, TextLayout};
use crate::text::{Edit, EditEngine, EditableText, EditorMetrics, Motion};

/// How many changes can be undone.
const HISTORY_LIMIT: usize = 100;

/// The text and selection an undo or a redo returns to.
#[derive(Clone)]
struct Snapshot {
    text: String,
    selection: Selection,
}

/// Text an input method is composing, laid out in the text in place of the selection it began
/// at.
struct Composition {
    /// The text and selection before composing began, which cancelling returns to.
    original: Snapshot,
    /// Where the composing text is in the text.
    range: Range<usize>,
    /// Whether the input method shows a caret in the composing text.
    caret_visible: bool,
}

/// The text of an `<input>` or `<textarea>`, laid out by winkin.
pub struct TextEditor {
    /// The text, with any text an input method is composing.
    text: String,
    selection: Selection,
    is_multiline: bool,
    /// The control, and its computed style, which the text is set in.
    style: Option<(NodeId, ServoArc<ComputedValues>)>,
    scale: f32,
    /// The width lines wrap at in a `<textarea>`, in device pixels.
    width: Option<f32>,
    layout: TextLayout,
    /// Whether a setter changed what the text is laid out with since it was last laid out.
    dirty: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    composition: Option<Composition>,
}

impl TextEditor {
    /// The laid-out text, which the renderer paints, once it is laid out.
    pub fn text_layout(&self) -> &TextLayout {
        &self.layout
    }

    /// The selection, in bytes of the text.
    pub fn selection_range(&self) -> Range<usize> {
        self.selection.range()
    }

    /// Where the text an input method is composing is, in bytes of the text, while it composes.
    pub fn composition_range(&self) -> Option<Range<usize>> {
        self.composition
            .as_ref()
            .map(|composition| composition.range.clone())
    }

    /// Lays the text out again in the control's style.
    fn relayout(&mut self, cx: &mut TextContext) {
        self.dirty = false;
        let Some((node, style)) = &self.style else {
            return;
        };
        let width = if self.is_multiline { self.width } else { None };
        let composition = self.composition_range();
        super::build_plain_text(
            cx,
            &mut self.layout,
            *node,
            style,
            &self.text,
            composition,
            self.scale,
            width,
        );
    }

    /// Clamps the selection to the text, at character boundaries.
    fn clamp_selection(&mut self) {
        let clamp = |position: Position| {
            let mut offset = position.offset.min(self.text.len());
            while !self.text.is_char_boundary(offset) {
                offset -= 1;
            }
            Position::new(offset, position.affinity)
        };
        self.selection = Selection::new(
            clamp(self.selection.anchor()),
            clamp(self.selection.focus()),
        );
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            selection: self.selection,
        }
    }

    /// Returns the text and the selection to `snapshot`, and lays the text out again.
    fn restore(&mut self, cx: &mut TextContext, snapshot: Snapshot) {
        self.text = snapshot.text;
        self.selection = snapshot.selection;
        self.clamp_selection();
        self.relayout(cx);
    }

    /// Records `before` as the state an undo returns to, which drops what could be redone.
    fn remember(&mut self, before: Snapshot) {
        if self.undo.len() == HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }

    /// Drops the history, as a change of the text from outside the editor does.
    fn forget(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Removes the text an input method is composing, returning the text and the selection to
    /// what they were before it began.
    fn cancel_composition(&mut self, cx: &mut TextContext) {
        if let Some(composition) = self.composition.take() {
            self.restore(cx, composition.original);
        }
    }

    /// Replaces the selection with `text`, leaving a caret after it, as one change to undo.
    fn replace_selection(&mut self, cx: &mut TextContext, text: &str) {
        self.cancel_composition(cx);
        let range = self.selection.range();
        if range.is_empty() && text.is_empty() {
            return;
        }
        let before = self.snapshot();
        self.replace(cx, range, text, before);
    }

    /// Replaces `range` with `text`, leaving a caret after it, as one change, which an undo
    /// takes back to `before`.
    fn replace(&mut self, cx: &mut TextContext, range: Range<usize>, text: &str, before: Snapshot) {
        self.remember(before);
        self.text.replace_range(range.clone(), text);
        self.selection = Selection::from(Position::from(range.start + text.len()));
        self.relayout(cx);
    }

    /// Deletes the selection or, where it is a caret, what `granularity` takes in `direction`.
    fn delete(
        &mut self,
        cx: &mut TextContext,
        direction: MotionDirection,
        granularity: Granularity,
    ) {
        self.cancel_composition(cx);
        let before = self.snapshot();
        if self.selection.is_collapsed() {
            self.modify(direction.extending(granularity));
        }
        let range = self.selection.range();
        if range.is_empty() {
            self.selection = before.selection;
        } else {
            self.replace(cx, range, "", before);
        }
    }

    /// Lays out `text` as what an input method is composing, in place of the selection or of the
    /// text it composed before, with the selection `cursor` sets inside it.
    fn compose(&mut self, cx: &mut TextContext, text: &str, cursor: Option<(usize, usize)>) {
        if text.is_empty() {
            self.cancel_composition(cx);
            return;
        }
        let composition = self.composition.take().unwrap_or_else(|| Composition {
            original: self.snapshot(),
            range: self.selection.range(),
            caret_visible: true,
        });
        let start = composition.range.start;
        self.text.replace_range(composition.range, text);
        // The input method's offsets are bytes of the composing text.
        let boundary = |offset: usize| {
            let mut offset = offset.min(text.len());
            while !text.is_char_boundary(offset) {
                offset -= 1;
            }
            start + offset
        };
        let (anchor, focus) = cursor.unwrap_or((text.len(), text.len()));
        self.selection = Selection::new(
            Position::from(boundary(anchor)),
            Position::from(boundary(focus)),
        );
        self.composition = Some(Composition {
            original: composition.original,
            range: start..start + text.len(),
            caret_visible: cursor.is_some(),
        });
        self.relayout(cx);
    }

    /// Returns to the state before the last change, keeping the present one to redo.
    fn undo(&mut self, cx: &mut TextContext) {
        self.cancel_composition(cx);
        if let Some(snapshot) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(cx, snapshot);
        }
    }

    /// Makes the last change undone again.
    fn redo(&mut self, cx: &mut TextContext) {
        self.cancel_composition(cx);
        if let Some(snapshot) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(cx, snapshot);
        }
    }

    /// Moves or extends the selection by `motion`, where the text is laid out.
    fn modify(&mut self, motion: winkin::selection::Motion) {
        if let Some(layout) = self.layout.layout() {
            self.selection.modify(layout, motion);
        }
    }

    /// The text position nearest the point, where the text is laid out.
    fn hit(&self, x: f32, y: f32) -> Option<Position> {
        self.layout.layout()?.hit_test(x, y, PastLines::Column)
    }

    /// The byte range of what is around `offset` up to the nearest boundaries `is_boundary`
    /// finds on either side.
    fn expand(&self, offset: usize, is_boundary: impl Fn(char) -> bool) -> Range<usize> {
        let start = self.text[..offset]
            .char_indices()
            .rev()
            .find(|(_, ch)| is_boundary(*ch))
            .map_or(0, |(at, ch)| at + ch.len_utf8());
        let end = self.text[offset..]
            .char_indices()
            .find(|(_, ch)| is_boundary(*ch))
            .map_or(self.text.len(), |(at, _)| offset + at);
        start..end
    }
}

/// Where `motion` moves a caret, as winkin's selection motions do it.
fn motion(motion: Motion, extend: bool) -> winkin::selection::Motion {
    use MotionDirection::{Backward, Forward, Left, Right};
    let (direction, granularity) = match motion {
        Motion::Left => (Left, Granularity::Character),
        Motion::Right => (Right, Granularity::Character),
        Motion::WordLeft => (Left, Granularity::Word),
        Motion::WordRight => (Right, Granularity::Word),
        Motion::Up => (Backward, Granularity::Line),
        Motion::Down => (Forward, Granularity::Line),
        Motion::LineStart => (Backward, Granularity::LineBoundary),
        Motion::LineEnd => (Forward, Granularity::LineBoundary),
        Motion::HardLineStart => (Backward, Granularity::ParagraphBoundary),
        Motion::HardLineEnd => (Forward, Granularity::ParagraphBoundary),
        Motion::TextStart => (Backward, Granularity::DocumentBoundary),
        Motion::TextEnd => (Forward, Granularity::DocumentBoundary),
    };
    if extend {
        direction.extending(granularity)
    } else {
        direction.moving(granularity)
    }
}

impl EditableText for TextEditor {
    fn raw_text(&self) -> &str {
        &self.text
    }

    fn text(&self) -> Cow<'_, str> {
        match &self.composition {
            Some(composition) => {
                let mut text = self.text.clone();
                text.replace_range(composition.range.clone(), "");
                Cow::Owned(text)
            }
            None => Cow::Borrowed(&self.text),
        }
    }

    fn selection(&self) -> Range<usize> {
        self.selection.range()
    }

    fn selected_text(&self) -> Option<&str> {
        let range = self.selection.range();
        (self.composition.is_none() && !range.is_empty()).then(|| &self.text[range])
    }

    fn is_selection_collapsed(&self) -> bool {
        self.selection.is_collapsed()
    }

    fn metrics(&self) -> Option<EditorMetrics> {
        let layout = self.layout.layout().filter(|_| !self.dirty)?;
        let width = layout
            .lines()
            .map(|line| {
                let metrics = line.metrics();
                metrics.left + metrics.width
            })
            .fold(0.0, f32::max);
        Some(EditorMetrics {
            size: Size::new(f64::from(width), f64::from(layout.metrics().block_end)),
            scale: self.scale,
        })
    }

    fn caret_rect(&self) -> Option<Rect> {
        if self
            .composition
            .as_ref()
            .is_some_and(|composition| !composition.caret_visible)
        {
            return None;
        }
        let layout = self.layout.layout().filter(|_| !self.dirty)?;
        let caret = layout.caret(self.selection.focus())?;
        let metrics = layout.line(caret.line)?.metrics();
        let x = f64::from(metrics.left + caret.inline.left);
        Some(Rect::new(
            x,
            f64::from(metrics.top + caret.block.over),
            x + 1.5,
            f64::from(metrics.top + caret.block.under),
        ))
    }
}

impl EditEngine for TextEditor {
    fn new(is_multiline: bool) -> Self {
        Self {
            text: String::new(),
            selection: Selection::default(),
            is_multiline,
            style: None,
            scale: 1.0,
            width: None,
            layout: TextLayout::default(),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            composition: None,
        }
    }

    fn set_initial_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.selection = Selection::from(Position::from(text.len()));
        self.composition = None;
        self.forget();
    }

    fn set_text(&mut self, text: &str) {
        if self.text != text {
            self.text = text.to_string();
            self.composition = None;
            self.forget();
            self.clamp_selection();
            self.dirty = true;
        }
    }

    fn set_style(&mut self, node: NodeId, style: Option<&ComputedValues>, scale: f32) {
        self.style = style.map(|style| (node, ServoArc::new(style.clone())));
        self.scale = scale;
        self.width = None;
        self.dirty = true;
    }

    fn set_scale(&mut self, scale: f32) {
        if self.scale != scale {
            self.scale = scale;
            self.dirty = true;
        }
    }

    fn set_width(&mut self, width: Option<f32>) {
        if self.width != width {
            self.width = width;
            self.dirty |= self.is_multiline;
        }
    }

    fn refresh(&mut self, cx: &mut TextContext) {
        if self.dirty {
            self.relayout(cx);
        }
    }

    fn edit(&mut self, cx: &mut TextContext, edit: Edit<'_>) {
        self.refresh(cx);
        // Composing ends where anything but the input method changes the text or the selection.
        if !matches!(edit, Edit::SetCompose(..)) {
            self.cancel_composition(cx);
        }
        match edit {
            Edit::Insert(text) => self.replace_selection(cx, text),
            Edit::Delete => self.delete(cx, MotionDirection::Forward, Granularity::Character),
            Edit::Backdelete => self.delete(cx, MotionDirection::Backward, Granularity::Character),
            Edit::DeleteWord => self.delete(cx, MotionDirection::Forward, Granularity::Word),
            Edit::BackdeleteWord => self.delete(cx, MotionDirection::Backward, Granularity::Word),
            Edit::DeleteSelection => self.replace_selection(cx, ""),
            Edit::Move(to) => self.modify(motion(to, false)),
            Edit::Extend(to) => self.modify(motion(to, true)),
            Edit::Select(from, to) => {
                self.modify(motion(from, false));
                self.modify(motion(to, true));
            }
            Edit::SelectAll => {
                self.selection = Selection::new(Position::from(0), Position::from(self.text.len()));
            }
            Edit::CollapseSelection => {
                self.selection = Selection::from(self.selection.focus());
            }
            Edit::SelectByteRange(start, end) => {
                self.selection = Selection::new(Position::from(start), Position::from(end));
                self.clamp_selection();
            }
            Edit::SetCompose(text, cursor) => self.compose(cx, text, cursor),
            // Cancelled above.
            Edit::ClearCompose => {}
            Edit::Undo => self.undo(cx),
            Edit::Redo => self.redo(cx),
            Edit::MoveToPoint(x, y) => {
                if let Some(position) = self.hit(x, y) {
                    self.selection = Selection::from(position);
                }
            }
            Edit::ExtendSelectionToPoint(x, y) | Edit::ShiftClickExtension(x, y) => {
                if let Some(position) = self.hit(x, y) {
                    self.selection = Selection::new(self.selection.anchor(), position);
                }
            }
            Edit::SelectWordAtPoint(x, y) => {
                if let Some(position) = self.hit(x, y) {
                    let range = self.expand(position.offset, |ch| !ch.is_alphanumeric());
                    self.selection =
                        Selection::new(Position::from(range.start), Position::from(range.end));
                }
            }
            Edit::SelectHardLineAtPoint(x, y) => {
                if let Some(position) = self.hit(x, y) {
                    let range = self.expand(position.offset, |ch| ch == '\n');
                    self.selection =
                        Selection::new(Position::from(range.start), Position::from(range.end));
                }
            }
        }
    }
}
