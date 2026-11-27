use std::time::Duration;

use floem_editor_core::{
    buffer::{Buffer, InvalLines, rope_text::RopeText},
    command::EditCommand,
    cursor::{ColPosition, Cursor, CursorAffinity, CursorMode},
    editor::{Action, EditConf, EditType},
    mode::{Mode, VisualMode},
    register::{Clipboard, Register, RegisterData},
    selection::{SelRegion, Selection},
    word::WordCursor,
    xi_rope::{Rope, RopeDelta},
};
use kurbo::{Point, Rect, Size, Vec2};

use crate::highlight::{HighlightCache, SyntaxHighlighter};
use crate::layout::{LayoutConfig, LineCache, Metrics, caret_point, col_at_point};
use crate::{
    EditorInput, EditorStyle, ImeInput, Key, KeyInput, Modifiers, PointerButton, ScrollDelta,
    TextContext, Theme,
};
use multi_selection::MultiSelectCommand;

mod multi_selection;

/// Lines scrolled per wheel "line" step
const WHEEL_LINES: f64 = 3.0;

/// How long the caret stays visible (or hidden) while blinking
const DEFAULT_CARET_BLINK: Duration = Duration::from_millis(530);

/// How often to scroll while drag-selecting with the pointer outside the editor
const AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(16);

/// What the host should do after the editor handled an input
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorResponse {
    /// The editor used the input. If `false`, the host can apply its own default handling
    /// (e.g. scroll an ancestor, or move focus on Tab).
    pub handled: bool,
    /// The editor needs to be repainted
    pub redraw: bool,
    /// The text changed
    pub text_changed: bool,
    /// Put this text on the system clipboard
    pub set_clipboard: Option<String>,
    /// Read the system clipboard and send its text back as [`EditorInput::Paste`]
    pub request_clipboard: bool,
    /// Enable or disable the IME
    pub ime_enabled: Option<bool>,
    /// Where the IME candidate window should be placed (the caret), in editor coordinates
    pub ime_cursor_area: Option<Rect>,
    /// Send [`EditorInput::Timer`] after this delay, replacing any timer requested earlier
    pub timer: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Movement {
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    PageUp,
    PageDown,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
}

enum KeyAction {
    Move(Movement),
    Edit(EditCommand),
    MultiSelect(MultiSelectCommand),
    SelectAll,
    Paste,
    Collapse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Granularity {
    Char,
    Word,
    Line,
}

/// An in-progress pointer drag selection
#[derive(Clone, Copy, Debug)]
struct Drag {
    granularity: Granularity,
    /// The range selected by the initial click (a word or line for multi-clicks)
    anchor: (usize, usize),
    /// The latest pointer position
    pos: Point,
}

/// IME composition text, shown at the primary caret but not yet part of the buffer
#[derive(Clone, Debug)]
pub(crate) struct Preedit {
    pub text: String,
    pub cursor: Option<(usize, usize)>,
}

/// Collects text put on the clipboard by editor-core commands, so it can be returned to the
/// host. Reads go through [`EditorResponse::request_clipboard`] instead, since system
/// clipboards are usually asynchronous.
#[derive(Default)]
struct CapturedClipboard(Option<String>);

impl Clipboard for CapturedClipboard {
    fn get_string(&mut self) -> Option<String> {
        None
    }

    fn put_string(&mut self, s: impl AsRef<str>) {
        self.0 = Some(s.as_ref().to_owned());
    }
}

/// A code editing control
pub struct CodeEditor {
    pub(crate) buffer: Buffer,
    cursor: Cursor,
    register: Register,
    pub(crate) style: EditorStyle,
    pub(crate) theme: Theme,
    pub(crate) cache: LineCache,
    pub(crate) metrics: Metrics,
    /// The style `metrics` were measured for
    measured_style: Option<EditorStyle>,
    pub(crate) size: Size,
    pub(crate) scroll: Vec2,
    pub(crate) focused: bool,
    pub(crate) preedit: Option<Preedit>,
    drag: Option<Drag>,
    caret_blink: Option<Duration>,
    /// Whether the caret is in the visible half of its blink cycle
    pub(crate) caret_visible: bool,
}

impl CodeEditor {
    pub fn new(text: &str) -> Self {
        Self {
            buffer: Buffer::new(text),
            cursor: Cursor::origin(false),
            register: Register::default(),
            style: EditorStyle::default(),
            theme: Theme::default(),
            cache: LineCache::new(),
            metrics: Metrics {
                line_height: 0.0,
                char_width: 0.0,
            },
            measured_style: None,
            size: Size::ZERO,
            scroll: Vec2::ZERO,
            focused: false,
            preedit: None,
            drag: None,
            caret_blink: Some(DEFAULT_CARET_BLINK),
            caret_visible: true,
        }
    }

    pub fn rope(&self) -> &Rope {
        self.buffer.text()
    }

    pub fn text(&self) -> String {
        self.buffer.text().to_string()
    }

    /// Replaces the whole text, clearing the undo history and selection
    pub fn set_text(&mut self, text: &str) {
        self.buffer = Buffer::new(text);
        self.cursor = Cursor::origin(false);
        self.cache.reset(self.buffer.num_lines());
        self.scroll = Vec2::ZERO;
        self.preedit = None;
        self.drag = None;
    }

    pub fn style(&self) -> &EditorStyle {
        &self.style
    }

    pub fn set_style(&mut self, style: EditorStyle) {
        self.style = style;
    }

    /// Highlights the text with `highlighter`
    pub fn set_highlighter(&mut self, highlighter: impl SyntaxHighlighter + 'static) {
        let num_lines = self.buffer.num_lines();
        let highlights = HighlightCache::new(highlighter, num_lines);
        self.cache
            .set_highlights(Some(Box::new(highlights)), num_lines);
    }

    /// Stops highlighting the text
    pub fn clear_highlighter(&mut self) {
        self.cache.set_highlights(None, self.buffer.num_lines());
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Sets the size of the editor in logical pixels
    pub fn set_size(&mut self, size: Size) {
        self.size = size;
        self.clamp_scroll();
    }

    pub fn scroll_offset(&self) -> Vec2 {
        self.scroll
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Sets how long the caret stays visible (and hidden) while blinking. `None` disables
    /// blinking.
    pub fn set_caret_blink(&mut self, interval: Option<Duration>) {
        self.caret_blink = interval;
        self.caret_visible = true;
    }

    /// The selected byte ranges, as `(anchor, caret)` pairs
    pub fn selections(&self) -> Vec<(usize, usize)> {
        self.selection()
            .regions()
            .iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    /// The byte offset of the primary caret
    pub fn caret(&self) -> usize {
        self.selection().get_cursor_offset()
    }

    pub fn handle_input(&mut self, cx: &mut TextContext, input: EditorInput) -> EditorResponse {
        self.prepare(cx);
        let mut resp = EditorResponse::default();
        // Scrolling doesn't move the caret, so it doesn't restart the blink cycle
        let restarts_blink = !matches!(input, EditorInput::Wheel { .. } | EditorInput::Timer);
        match input {
            EditorInput::Key(key) => self.handle_key(cx, key, &mut resp),
            EditorInput::Ime(ime) => self.handle_ime(cx, ime, &mut resp),
            EditorInput::PointerDown {
                pos,
                button: PointerButton::Primary,
                mods,
                click_count,
            } => self.pointer_down(cx, pos, mods, click_count, &mut resp),
            EditorInput::PointerDown { .. } => {}
            EditorInput::PointerMove { pos } => self.pointer_move(cx, pos, &mut resp),
            EditorInput::PointerUp { .. } => {
                resp.handled = self.drag.take().is_some();
            }
            EditorInput::Wheel { delta } => self.wheel(delta, &mut resp),
            EditorInput::Paste(text) => {
                self.paste(text, &mut resp);
                self.scroll_to_caret(cx);
            }
            EditorInput::Focus(focused) => {
                self.focused = focused;
                if !focused {
                    self.preedit = None;
                    self.drag = None;
                }
                resp.ime_enabled = Some(focused);
                resp.handled = true;
                resp.redraw = true;
            }
            EditorInput::Timer => {
                if !self.autoscroll(cx, &mut resp) {
                    self.blink(&mut resp);
                }
            }
        }
        if self.focused && resp.redraw && restarts_blink {
            resp.ime_cursor_area = Some(self.caret_rect(cx));
            // Show the caret straight away after it moves or the text changes
            self.caret_visible = true;
            resp.timer = self.caret_blink;
        }
        // Start scrolling when a drag selection moves the pointer outside the editor. The timer
        // keeps itself going from there (see `autoscroll`) until the scroll offset stops moving.
        if restarts_blink
            && let Some(drag) = self.drag
            && self.autoscroll_step(drag.pos) != Vec2::ZERO
        {
            resp.timer = Some(AUTOSCROLL_INTERVAL);
        }
        resp
    }

    fn blink(&mut self, resp: &mut EditorResponse) {
        let Some(interval) = self.caret_blink.filter(|_| self.focused) else {
            return;
        };
        // Keep the caret steady while composing or dragging a selection
        self.caret_visible = !self.caret_visible || self.preedit.is_some() || self.drag.is_some();
        resp.handled = true;
        resp.redraw = true;
        resp.timer = Some(interval);
    }

    /// Measures the font and syncs the layout cache with the current style and theme
    pub(crate) fn prepare(&mut self, cx: &mut TextContext) {
        if self.measured_style.as_ref() != Some(&self.style) {
            self.metrics = Metrics::measure(cx, &self.style);
            self.measured_style = Some(self.style.clone());
        }
        // Leave a column free at the right edge for the caret. Wrapping is off until the editor
        // has a size, rather than wrapping at every character.
        let wrap_width = (self.style.soft_wrap && self.size.width > 0.0)
            .then(|| {
                (self.text_area_width() - self.metrics.char_width).max(self.metrics.char_width)
            })
            .map(|width| width as f32);
        let config = LayoutConfig {
            style: self.style.clone(),
            metrics: self.metrics,
            color: self.theme.foreground,
            wrap_width,
        };
        if self.cache.configure(config, self.buffer.num_lines()) {
            self.clamp_scroll();
        }
    }

    pub(crate) fn selection(&self) -> Selection {
        match &self.cursor.mode {
            CursorMode::Insert(selection) => selection.clone(),
            mode => Selection::caret(mode.offset(), mode.affinity()),
        }
    }

    /// Replaces the selection. Unlike `Cursor::set_insert`, this doesn't record the previous
    /// selection in `history_selections`, which would otherwise grow on every caret move.
    fn set_selection(&mut self, selection: Selection) {
        self.cursor.mode = CursorMode::Insert(selection);
    }

    // --- Geometry

    pub(crate) fn gutter_width(&self) -> f64 {
        if !self.style.show_gutter {
            return 0.0;
        }
        let digits = self.buffer.num_lines().to_string().len().max(2);
        (digits + 3) as f64 * self.metrics.char_width
    }

    /// The x position of column 0 of the text, before scrolling
    pub(crate) fn text_x(&self) -> f64 {
        self.gutter_width() + self.metrics.char_width
    }

    fn text_area_width(&self) -> f64 {
        (self.size.width - self.text_x()).max(0.0)
    }

    // Text coordinates have their origin at the start of the first line, before scrolling.
    // Lines are a whole number of rows tall: more than one when they soft wrap.

    /// The top of `line` in text coordinates
    pub(crate) fn line_top(&self, line: usize) -> f64 {
        self.cache.rows().before(line) as f64 * self.metrics.line_height
    }

    /// The top-left corner of the caret at `offset`, in text coordinates
    fn offset_point(&mut self, cx: &mut TextContext, offset: usize) -> Point {
        let line = self.buffer.line_of_offset(offset);
        let col = offset - self.buffer.offset_of_line(line);
        let point = caret_point(self.cache.line(cx, &self.buffer, line), col);
        Point::new(point.x, self.line_top(line) + point.y)
    }

    /// The row (counting from the top of the text) of the caret at `offset`
    fn offset_row(&mut self, cx: &mut TextContext, offset: usize) -> usize {
        (self.offset_point(cx, offset).y / self.metrics.line_height).round() as usize
    }

    /// The offset closest to a point in text coordinates
    fn offset_at_text_point(&mut self, cx: &mut TextContext, point: Point) -> usize {
        let lh = self.metrics.line_height;
        let (line, row) = self.cache.rows().line_at((point.y.max(0.0) / lh) as usize);
        let line_start = self.buffer.offset_of_line(line);
        let layout = self.cache.line(cx, &self.buffer, line);
        let col = col_at_point(layout, Point::new(point.x, (row as f64 + 0.5) * lh));
        (line_start + col).min(self.buffer.line_end_offset(line, true))
    }

    /// The offset closest to a point in editor coordinates
    fn offset_at_point(&mut self, cx: &mut TextContext, pos: Point) -> usize {
        let point = Point::new(pos.x - self.text_x(), pos.y) + self.scroll;
        self.offset_at_text_point(cx, point)
    }

    /// The primary caret's rectangle in editor coordinates
    fn caret_rect(&mut self, cx: &mut TextContext) -> Rect {
        let offset = self.caret();
        let mut point = self.offset_point(cx, offset);
        if let Some(preedit) = &self.preedit {
            let line = self.buffer.line_of_offset(offset);
            let col = offset - self.buffer.offset_of_line(line);
            let ime_col = col + preedit.cursor.map_or(preedit.text.len(), |c| c.1);
            let text = preedit.text.clone();
            let layout = self
                .cache
                .line_with_preedit(cx, &self.buffer, line, col, &text);
            point = caret_point(&layout, ime_col) + Vec2::new(0.0, self.line_top(line));
        }
        let origin = Point::new(self.text_x() + point.x, point.y) - self.scroll;
        Rect::from_origin_size(origin, Size::new(1.0, self.metrics.line_height))
    }

    fn page_rows(&self) -> usize {
        ((self.size.height / self.metrics.line_height) as usize)
            .saturating_sub(1)
            .max(1)
    }

    // --- Scrolling

    fn max_scroll(&self) -> Vec2 {
        let lh = self.metrics.line_height;
        // Allow scrolling the last row to the top of the viewport
        let y = self.cache.rows().total().saturating_sub(1) as f64 * lh;
        let x = if self.style.soft_wrap {
            0.0
        } else {
            let content_width = self.cache.max_width() + 2.0 * self.metrics.char_width;
            (content_width - self.text_area_width()).max(0.0)
        };
        Vec2::new(x, y)
    }

    fn clamp_scroll(&mut self) {
        let max = self.max_scroll();
        self.scroll.x = self.scroll.x.clamp(0.0, max.x);
        self.scroll.y = self.scroll.y.clamp(0.0, max.y);
    }

    fn scroll_to_offset(&mut self, cx: &mut TextContext, offset: usize) {
        let lh = self.metrics.line_height;
        let Point { x, y: top } = self.offset_point(cx, offset);
        if top < self.scroll.y {
            self.scroll.y = top;
        } else if top + lh > self.scroll.y + self.size.height {
            self.scroll.y = top + lh - self.size.height;
        }

        let margin = 2.0 * self.metrics.char_width;
        let width = self.text_area_width();
        if x - margin < self.scroll.x {
            self.scroll.x = x - margin;
        } else if x + margin > self.scroll.x + width {
            self.scroll.x = x + margin - width;
        }
        self.clamp_scroll();
    }

    fn scroll_to_caret(&mut self, cx: &mut TextContext) {
        self.scroll_to_offset(cx, self.caret());
    }

    fn wheel(&mut self, delta: ScrollDelta, resp: &mut EditorResponse) {
        let (dx, dy) = match delta {
            ScrollDelta::Lines(x, y) => (
                x * WHEEL_LINES * self.metrics.char_width,
                y * WHEEL_LINES * self.metrics.line_height,
            ),
            ScrollDelta::Pixels(x, y) => (x, y),
        };
        let before = self.scroll;
        self.scroll -= Vec2::new(dx, dy);
        self.clamp_scroll();
        resp.handled = self.scroll != before;
        resp.redraw = resp.handled;
    }

    // --- Editing

    fn edit_conf(&self) -> EditConf<'static> {
        EditConf {
            comment_token: "//",
            modal: false,
            smart_tab: true,
            keep_indent: true,
            auto_indent: true,
        }
    }

    fn apply_deltas(
        &mut self,
        deltas: &[(Rope, RopeDelta, InvalLines)],
        resp: &mut EditorResponse,
    ) {
        let num_lines = self.buffer.num_lines();
        for (_, _, inval) in deltas {
            self.cache.invalidate(inval, num_lines);
        }
        if !deltas.is_empty() {
            // Earlier selections refer to offsets in the old text
            self.cursor.history_selections.clear();
            resp.text_changed = true;
        }
    }

    fn run_command(&mut self, cmd: EditCommand, resp: &mut EditorResponse) {
        let mut clipboard = CapturedClipboard::default();
        let conf = self.edit_conf();
        let deltas = Action::do_edit(
            &mut self.cursor,
            &mut self.buffer,
            &cmd,
            &mut clipboard,
            &mut self.register,
            conf,
        );
        self.apply_deltas(&deltas, resp);
        resp.set_clipboard = clipboard.0;
    }

    fn insert(&mut self, text: &str, resp: &mut EditorResponse) {
        let deltas = Action::insert(
            &mut self.cursor,
            &mut self.buffer,
            text,
            &|buffer, c, offset| WordCursor::new(buffer.text(), offset).previous_unmatched(c),
            false,
            false,
        );
        self.apply_deltas(&deltas, resp);
    }

    fn paste(&mut self, content: String, resp: &mut EditorResponse) {
        let mode = if content.ends_with('\n') {
            VisualMode::Linewise
        } else {
            VisualMode::Normal
        };
        let data = RegisterData { content, mode };
        let deltas = Action::do_paste(&mut self.cursor, &mut self.buffer, &data);
        self.apply_deltas(&deltas, resp);
        resp.handled = true;
        resp.redraw = true;
    }

    fn selection_spans_lines(&self) -> bool {
        self.selection().regions().iter().any(|r| {
            !r.is_caret()
                && self.buffer.line_of_offset(r.min()) != self.buffer.line_of_offset(r.max())
        })
    }

    // --- Keyboard

    fn key_action(&self, key: &Key, mods: Modifiers) -> Option<KeyAction> {
        use KeyAction::{Edit, Move, MultiSelect};
        use Movement::*;
        let mac = cfg!(target_os = "macos");
        let primary = if mac { mods.meta } else { mods.ctrl };
        let word = if mac { mods.alt } else { mods.ctrl };
        Some(match key {
            Key::ArrowUp if primary && mods.alt => MultiSelect(MultiSelectCommand::CursorAbove),
            Key::ArrowDown if primary && mods.alt => MultiSelect(MultiSelectCommand::CursorBelow),
            Key::ArrowLeft if mac && mods.meta => Move(LineStart),
            Key::ArrowLeft if word => Move(WordLeft),
            Key::ArrowLeft => Move(Left),
            Key::ArrowRight if mac && mods.meta => Move(LineEnd),
            Key::ArrowRight if word => Move(WordRight),
            Key::ArrowRight => Move(Right),
            Key::ArrowUp if mac && mods.meta => Move(DocStart),
            Key::ArrowUp if mods.alt => Edit(EditCommand::MoveLineUp),
            Key::ArrowUp => Move(Up),
            Key::ArrowDown if mac && mods.meta => Move(DocEnd),
            Key::ArrowDown if mods.alt => Edit(EditCommand::MoveLineDown),
            Key::ArrowDown => Move(Down),
            Key::Home if mods.ctrl => Move(DocStart),
            Key::Home => Move(LineStart),
            Key::End if mods.ctrl => Move(DocEnd),
            Key::End => Move(LineEnd),
            Key::PageUp => Move(PageUp),
            Key::PageDown => Move(PageDown),
            Key::Backspace if mac && mods.meta => Edit(EditCommand::DeleteToBeginningOfLine),
            Key::Backspace if word => Edit(EditCommand::DeleteWordBackward),
            Key::Backspace => Edit(EditCommand::DeleteBackward),
            Key::Delete if word => Edit(EditCommand::DeleteWordForward),
            Key::Delete => Edit(EditCommand::DeleteForward),
            Key::Enter => Edit(EditCommand::InsertNewLine),
            Key::Tab if mods.shift => Edit(EditCommand::OutdentLine),
            Key::Tab if self.selection_spans_lines() => Edit(EditCommand::IndentLine),
            Key::Tab => Edit(EditCommand::InsertTab),
            Key::Escape => KeyAction::Collapse,
            Key::Character(c) if primary => match c.to_lowercase().as_str() {
                "z" if mods.shift => Edit(EditCommand::Redo),
                "z" => Edit(EditCommand::Undo),
                "y" if !mac => Edit(EditCommand::Redo),
                "a" => KeyAction::SelectAll,
                "c" => Edit(EditCommand::ClipboardCopy),
                "x" => Edit(EditCommand::ClipboardCut),
                "v" => KeyAction::Paste,
                "d" => MultiSelect(MultiSelectCommand::SelectNextOccurrence),
                "l" if mods.shift => MultiSelect(MultiSelectCommand::SelectAllOccurrences),
                "l" => MultiSelect(MultiSelectCommand::SelectLine),
                "u" => MultiSelect(MultiSelectCommand::Undo),
                _ => return None,
            },
            // Emacs-style line navigation, as in other macOS text views
            Key::Character(c) if mac && mods.ctrl => match c.to_lowercase().as_str() {
                "a" => Move(LineStart),
                "e" => Move(LineEnd),
                _ => return None,
            },
            _ => return None,
        })
    }

    fn handle_key(&mut self, cx: &mut TextContext, input: KeyInput, resp: &mut EditorResponse) {
        let KeyInput { key, mods, text } = input;
        match self.key_action(&key, mods) {
            Some(KeyAction::Move(movement)) => self.move_selection(cx, movement, mods.shift),
            Some(KeyAction::Edit(cmd)) => self.run_command(cmd, resp),
            Some(KeyAction::MultiSelect(cmd)) => self.multi_select(cx, cmd),
            Some(KeyAction::SelectAll) => {
                self.set_selection(Selection::region(
                    0,
                    self.buffer.len(),
                    CursorAffinity::Backward,
                ));
            }
            Some(KeyAction::Paste) => resp.request_clipboard = true,
            Some(KeyAction::Collapse) => {
                let selection = self.selection();
                if selection.len() <= 1 && selection.is_caret() {
                    return;
                }
                let caret = selection.get_cursor_offset();
                self.set_selection(Selection::caret(caret, selection.get_cursor_affinity()));
            }
            None => {
                // Ctrl+Alt is AltGr on Windows, which produces text
                let is_shortcut = mods.meta || (mods.ctrl && !mods.alt);
                match text {
                    Some(text)
                        if !is_shortcut
                            && self.preedit.is_none()
                            && !text.is_empty()
                            && !text.chars().any(char::is_control) =>
                    {
                        self.insert(&text, resp)
                    }
                    _ => return,
                }
            }
        }
        resp.handled = true;
        resp.redraw = true;
        self.scroll_to_caret(cx);
    }

    fn move_selection(&mut self, cx: &mut TextContext, movement: Movement, extend: bool) {
        self.buffer.reset_edit_type();
        let selection = self.selection();
        let mut new_selection = Selection::new();
        for region in selection.regions() {
            new_selection.add_region(self.move_region(cx, region, movement, extend));
        }
        self.set_selection(new_selection);
    }

    fn move_region(
        &mut self,
        cx: &mut TextContext,
        region: &SelRegion,
        movement: Movement,
        extend: bool,
    ) -> SelRegion {
        // Without shift, Left/Right collapse a selection to its start/end
        if !extend && !region.is_caret() {
            match movement {
                Movement::Left => return SelRegion::caret(region.min(), region.affinity),
                Movement::Right => return SelRegion::caret(region.max(), region.affinity),
                _ => {}
            }
        }

        let offset = region.end;
        let line = self.buffer.line_of_offset(offset);
        let mut horiz = None;
        let end = match movement {
            Movement::Left => self.buffer.move_left(offset, Mode::Insert, 1),
            Movement::Right => self.buffer.move_right(offset, Mode::Insert, 1),
            Movement::WordLeft => self.buffer.move_n_words_backward(offset, 1, Mode::Insert),
            Movement::WordRight => self.buffer.move_n_wordends_forward(offset, 1, true),
            Movement::LineStart => {
                // Toggle between the first non-blank character and the start of the line
                let first = self.buffer.first_non_blank_character_on_line(line);
                if offset == first {
                    self.buffer.offset_of_line(line)
                } else {
                    first
                }
            }
            Movement::LineEnd => self.buffer.line_end_offset(line, true),
            Movement::DocStart => 0,
            Movement::DocEnd => self.buffer.len(),
            Movement::Up | Movement::Down | Movement::PageUp | Movement::PageDown => {
                // Move by visual rows, so the caret steps through wrapped lines
                let point = self.offset_point(cx, offset);
                let x = match region.horiz {
                    Some(ColPosition::Col(x)) => x,
                    _ => point.x,
                };
                horiz = Some(ColPosition::Col(x));
                let delta = match movement {
                    Movement::Up => -1,
                    Movement::Down => 1,
                    Movement::PageUp => -(self.page_rows() as isize),
                    _ => self.page_rows() as isize,
                };
                let lh = self.metrics.line_height;
                let target = (point.y / lh).round() as isize + delta;
                if target < 0 {
                    0
                } else if target as usize >= self.cache.rows().total() {
                    self.buffer.len()
                } else {
                    self.offset_at_text_point(cx, Point::new(x, target as f64 * lh))
                }
            }
        };
        let start = if extend { region.start } else { end };
        SelRegion::new(start, end, region.affinity, horiz)
    }

    // --- Pointer

    fn granular_range(&self, offset: usize, granularity: Granularity) -> (usize, usize) {
        match granularity {
            Granularity::Char => (offset, offset),
            Granularity::Word => self.buffer.select_word(offset),
            Granularity::Line => {
                let line = self.buffer.line_of_offset(offset);
                (
                    self.buffer.offset_of_line(line),
                    self.buffer.offset_of_line(line + 1),
                )
            }
        }
    }

    fn pointer_down(
        &mut self,
        cx: &mut TextContext,
        pos: Point,
        mods: Modifiers,
        click_count: u8,
        resp: &mut EditorResponse,
    ) {
        self.buffer.reset_edit_type();
        let offset = self.offset_at_point(cx, pos);
        let granularity = match click_count {
            // Clicking line numbers selects whole lines
            _ if pos.x < self.gutter_width() => Granularity::Line,
            0 | 1 => Granularity::Char,
            2 => Granularity::Word,
            _ => Granularity::Line,
        };
        let (start, end) = self.granular_range(offset, granularity);
        let mut selection = self.selection();
        let affinity = selection.get_cursor_affinity();
        let anchor = if mods.shift && granularity == Granularity::Char {
            let anchor = selection.last_inserted().map_or(offset, |r| r.start);
            selection.replace_last_inserted_region(SelRegion::new(anchor, offset, affinity, None));
            (anchor, anchor)
        } else if mods.alt {
            selection.add_region(SelRegion::new(start, end, affinity, None));
            (start, end)
        } else {
            selection = Selection::region(start, end, affinity);
            (start, end)
        };
        self.set_selection(selection);
        self.drag = Some(Drag {
            granularity,
            anchor,
            pos,
        });
        resp.handled = true;
        resp.redraw = true;
    }

    fn pointer_move(&mut self, cx: &mut TextContext, pos: Point, resp: &mut EditorResponse) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        drag.pos = pos;
        self.extend_drag(cx);
        resp.handled = true;
        resp.redraw = true;
    }

    /// Moves the end of the dragged selection to the pointer
    fn extend_drag(&mut self, cx: &mut TextContext) {
        let Some(drag) = self.drag else {
            return;
        };
        let offset = self.offset_at_point(cx, drag.pos);
        let (start, end) = self.granular_range(offset, drag.granularity);
        let (anchor, focus) = if start < drag.anchor.0 {
            (drag.anchor.1, start)
        } else {
            (drag.anchor.0, end.max(drag.anchor.1))
        };
        let mut selection = self.selection();
        let affinity = selection.get_cursor_affinity();
        selection.replace_last_inserted_region(SelRegion::new(anchor, focus, affinity, None));
        self.set_selection(selection);
    }

    /// How far to scroll per [`AUTOSCROLL_INTERVAL`] while drag-selecting with the pointer at
    /// `pos`: faster the further the pointer is outside the text area
    fn autoscroll_step(&self, pos: Point) -> Vec2 {
        let max = 4.0 * self.metrics.line_height;
        let speed = |overshoot: f64| (overshoot / 2.0).clamp(1.0, max);
        let axis = |pos: f64, min: f64, max: f64| {
            if pos < min {
                -speed(min - pos)
            } else if pos > max {
                speed(pos - max)
            } else {
                0.0
            }
        };
        Vec2::new(
            axis(pos.x, self.gutter_width(), self.size.width),
            axis(pos.y, 0.0, self.size.height),
        )
    }

    /// Scrolls towards the pointer during a drag selection. Returns whether it scrolled.
    fn autoscroll(&mut self, cx: &mut TextContext, resp: &mut EditorResponse) -> bool {
        let Some(drag) = self.drag else {
            return false;
        };
        let before = self.scroll;
        self.scroll += self.autoscroll_step(drag.pos);
        self.clamp_scroll();
        if self.scroll == before {
            return false;
        }
        self.extend_drag(cx);
        resp.handled = true;
        resp.redraw = true;
        resp.timer = Some(AUTOSCROLL_INTERVAL);
        true
    }

    // --- IME

    fn handle_ime(&mut self, cx: &mut TextContext, ime: ImeInput, resp: &mut EditorResponse) {
        match ime {
            ImeInput::Enabled => {}
            ImeInput::Disabled => self.preedit = None,
            ImeInput::Preedit { text, cursor } => {
                self.preedit = (!text.is_empty()).then_some(Preedit { text, cursor });
            }
            ImeInput::Commit(text) => {
                self.preedit = None;
                self.insert(&text, resp);
            }
            ImeInput::DeleteSurrounding {
                before_bytes,
                after_bytes,
            } => {
                let caret = self.caret();
                let start = caret.saturating_sub(before_bytes);
                let end = (caret + after_bytes).min(self.buffer.len());
                let selection = Selection::region(start, end, CursorAffinity::Backward);
                let delta = self.buffer.edit([(&selection, "")], EditType::Delete);
                self.apply_deltas(&[delta], resp);
                self.set_selection(Selection::caret(start, CursorAffinity::Backward));
            }
        }
        resp.handled = true;
        resp.redraw = true;
        self.scroll_to_caret(cx);
    }
}

#[cfg(test)]
mod tests;
