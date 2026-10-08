use blitz_traits::{
    events::{BlitzImeEvent, BlitzKeyEvent},
    node_id::NodeId,
    shell::ShellProvider,
};
use keyboard_types::{Key, Modifiers};
use kurbo::{Rect, Size};
#[cfg(not(feature = "winkin"))]
use parley::{Affinity, BreakReason, Cluster, ClusterSide, Cursor, Selection};
use parley::{ContentWidths, FontContext, LayoutContext};

#[cfg(feature = "winkin")]
use winkin::config::PastLines;
#[cfg(feature = "winkin")]
use winkin::selection::CopyKind;

/// A position in an inline root's laid-out text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineTextHit {
    pub node_id: NodeId,
    pub byte_offset: usize,
}

/// A piece of an inline root's laid-out content, in logical order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineContent<'a> {
    /// Text of node `node_id`, starting `start` bytes into the layout text. The node is the
    /// text node under winkin and its parent element under Parley.
    Text {
        node_id: NodeId,
        start: usize,
        text: std::borrow::Cow<'a, str>,
    },
    /// An inline box.
    Box(NodeId),
}

use crate::util::ACTION_MOD;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
/// Parley Brush type for Blitz which contains the Blitz node id
pub struct TextBrush {
    /// The node id for the span
    pub id: NodeId,
}

impl TextBrush {
    pub(crate) fn from_id(id: NodeId) -> Self {
        Self { id }
    }
}

#[derive(Clone, Default)]
pub struct TextLayout {
    pub text: String,
    pub content_widths: Option<ContentWidths>,
    pub layout: parley::layout::Layout<TextBrush>,
    /// Block-axis offset (in CSS px) of the line boxes from the top of the container's content
    /// box, as applied by `align-content`.
    pub block_offset: f32,
    /// The same content laid out by winkin, which measuring, breaking and
    /// painting read in place of `layout`.
    #[cfg(feature = "winkin")]
    pub winkin: crate::text_winkin::WinkinText,
}

impl TextLayout {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn scale(&self) -> f32 {
        #[cfg(feature = "winkin")]
        {
            self.winkin.scale()
        }
        #[cfg(not(feature = "winkin"))]
        self.layout.scale()
    }

    pub fn content_widths(&mut self) -> ContentWidths {
        *self
            .content_widths
            .get_or_insert_with(|| self.layout.calculate_content_widths())
    }

    /// The byte length of the selected backend's collapsed layout text.
    pub fn text_len(&self) -> usize {
        #[cfg(feature = "winkin")]
        {
            self.winkin.layout().map_or(0, |layout| layout.text().len())
        }
        #[cfg(not(feature = "winkin"))]
        self.text.len()
    }

    /// Text for a selection in the selected backend's byte-offset space.
    pub fn selected_text(&self, start: usize, end: usize) -> Option<String> {
        #[cfg(feature = "winkin")]
        {
            let layout = self.winkin.layout()?;
            (start < end && end <= layout.text().len())
                .then(|| layout.selected_text(start..end, CopyKind::Text).to_string())
        }
        #[cfg(not(feature = "winkin"))]
        {
            self.text.get(start..end).map(str::to_owned)
        }
    }

    /// The laid-out text and inline boxes in logical order, as the selected backend placed them
    /// on its lines.
    pub fn logical_content(&self) -> impl Iterator<Item = InlineContent<'_>> {
        let mut content = Vec::new();
        #[cfg(feature = "winkin")]
        if let Some(layout) = self.winkin.layout() {
            use crate::text_winkin::{FIRST_LETTER_KEY, MARKER_KEY};
            use core::ops::Range;
            use winkin::Item;
            use winkin::selection::{Affinity, Position};
            let mut items = Vec::new();
            for line in layout.lines() {
                items.clear();
                items.extend(line.items().filter_map(|item| match item {
                    Item::Text(run) => Some((run.text_range(), run.key().0, false)),
                    Item::Atomic(atomic) => Some((atomic.text_range(), atomic.key().0, true)),
                    _ => None,
                }));
                items.sort_by_key(|(range, ..)| range.start);
                // What the runs leave of the line, such as a forced break or a space the line
                // wraps at, goes with the node the layout maps it to.
                let line_range = line.text_range();
                let mut cursor = line_range.start;
                let push_text = |content: &mut Vec<_>, key: Option<u64>, range: Range<usize>| {
                    let key = key.or_else(|| {
                        let position = Position::new(range.start, Affinity::Downstream);
                        layout.node_position(position).map(|node| node.key.0)
                    });
                    let Some(key) = key.filter(|key| key & (FIRST_LETTER_KEY | MARKER_KEY) == 0)
                    else {
                        return;
                    };
                    let text = layout
                        .selected_text(range.clone(), CopyKind::Text)
                        .to_string();
                    if !text.is_empty() {
                        content.push(InlineContent::Text {
                            node_id: NodeId::from_u64(key),
                            start: range.start,
                            text: text.into(),
                        });
                    }
                };
                for (range, key, atomic) in items.drain(..) {
                    if range.start > cursor {
                        push_text(&mut content, None, cursor..range.start);
                    }
                    if atomic {
                        if key & (FIRST_LETTER_KEY | MARKER_KEY) == 0 {
                            content.push(InlineContent::Box(NodeId::from_u64(key)));
                        }
                    } else {
                        push_text(&mut content, Some(key), range.clone());
                    }
                    cursor = cursor.max(range.end);
                }
                if line_range.end > cursor {
                    push_text(&mut content, None, cursor..line_range.end);
                }
            }
        }
        #[cfg(not(feature = "winkin"))]
        {
            let text = self.text.as_str();
            let mut boxes = self.layout.inline_boxes().peekable();
            let mut runs = Vec::new();
            for line in self.layout.lines() {
                // Runs are stored in visual order: restore logical order for bidi text
                runs.clear();
                runs.extend(line.runs());
                runs.sort_by_key(|run| run.text_range().start);
                for run in &runs {
                    for cluster in run.clusters() {
                        let range = cluster.text_range();
                        while let Some(ibox) = boxes.next_if(|ibox| ibox.index <= range.start) {
                            content.push(InlineContent::Box(NodeId::from_u64(ibox.id)));
                        }
                        content.push(InlineContent::Text {
                            node_id: cluster.style().brush.id,
                            start: range.start,
                            text: text[range].into(),
                        });
                    }
                }
            }
            content.extend(boxes.map(|ibox| InlineContent::Box(NodeId::from_u64(ibox.id))));
        }
        content.into_iter()
    }

    /// Where byte `offset` of text node `node_id`'s content falls in the layout text, where the
    /// selected backend maps source text to its layout (winkin). `None` otherwise: Parley's
    /// callers follow its white space collapsing and text transforms themselves.
    pub fn source_offset(&self, node_id: NodeId, offset: usize) -> Option<usize> {
        #[cfg(feature = "winkin")]
        {
            let layout = self.winkin.layout()?;
            let position = layout.position(
                winkin::NodeKey(node_id.as_u64()),
                offset,
                winkin::selection::Affinity::Downstream,
            )?;
            Some(position.offset)
        }
        #[cfg(not(feature = "winkin"))]
        {
            let _ = (node_id, offset);
            None
        }
    }

    /// Whether [`Self::source_offset`] answers for the selected backend.
    pub fn maps_source(&self) -> bool {
        cfg!(feature = "winkin")
    }

    /// Hit test physical CSS-pixel coordinates relative to the inline root's content box.
    /// `exact` requires the point to be within a line and its text extent.
    pub fn hit_test(
        &self,
        x: f32,
        y: f32,
        content_size: Size,
        scale: f32,
        exact: bool,
    ) -> Option<InlineTextHit> {
        #[cfg(feature = "winkin")]
        {
            let layout = self.winkin.layout()?;
            let page = crate::text_winkin::page(
                self.winkin.writing_mode(),
                content_size.width * f64::from(scale),
                content_size.height * f64::from(scale),
            );
            let point = page.inverse()
                * kurbo::Point::new(
                    f64::from(x * scale),
                    f64::from((y - self.block_offset) * scale),
                );
            let (inline, block) = (point.x as f32, point.y as f32);
            if exact
                && !layout.lines().any(|line| {
                    let metrics = line.metrics();
                    block >= metrics.top
                        && block < metrics.top + metrics.height()
                        && inline >= metrics.left
                        && inline < metrics.left + metrics.width
                })
            {
                return None;
            }
            let position = layout.hit_test(inline, block, PastLines::Column)?;
            let node = layout.node_position(position)?;
            let node_id = NodeId::from_u64(node.key.0);
            Some(InlineTextHit {
                node_id,
                byte_offset: position.offset,
            })
        }
        #[cfg(not(feature = "winkin"))]
        {
            let _ = (content_size, scale);
            let layout = &self.layout;
            let point = (x * layout.scale(), (y - self.block_offset) * layout.scale());
            let (cluster, side) = if exact {
                Cluster::from_point_exact(layout, point.0, point.1)?
            } else {
                Cluster::from_point(layout, point.0, point.1)?
            };
            let leading = side == ClusterSide::Left;
            let byte_offset = if cluster.is_rtl() {
                if leading {
                    cluster.text_range().end
                } else {
                    cluster.text_range().start
                }
            } else if leading || cluster.is_line_break() == Some(BreakReason::Explicit) {
                cluster.text_range().start
            } else {
                cluster.text_range().end
            };
            Some(InlineTextHit {
                node_id: cluster.style().brush.id,
                byte_offset,
            })
        }
    }

    /// Selection rectangles in physical device pixels relative to the content box.
    pub fn selection_rects(
        &self,
        start: usize,
        end: usize,
        content_size: Size,
        scale: f32,
    ) -> Vec<Rect> {
        #[cfg(feature = "winkin")]
        {
            let Some(layout) = self.winkin.layout() else {
                return Vec::new();
            };
            let mode = self.winkin.writing_mode();
            let page = crate::text_winkin::page(
                mode,
                content_size.width * f64::from(scale),
                content_size.height * f64::from(scale),
            );
            layout
                .selection_rects(start..end)
                .filter_map(|selection| {
                    let metrics = layout.line(selection.line)?.metrics();
                    let rect = Rect::new(
                        f64::from(selection.inline.left),
                        f64::from(selection.block.over),
                        f64::from(selection.inline.right),
                        f64::from(selection.block.under),
                    );
                    Some(
                        crate::text_winkin::line_frame(mode, page, &metrics)
                            .transform_rect_bbox(rect),
                    )
                })
                .collect()
        }
        #[cfg(not(feature = "winkin"))]
        {
            let _ = (content_size, scale);
            let layout = &self.layout;
            let anchor = Cursor::from_byte_index(layout, start, Affinity::Downstream);
            let focus = Cursor::from_byte_index(layout, end, Affinity::Downstream);
            let selection = Selection::new(anchor, focus);
            let mut rects = Vec::new();
            selection.geometry_with(layout, |rect, _| {
                rects.push(Rect::new(rect.x0, rect.y0, rect.x1, rect.y1));
            });
            rects
        }
    }
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextLayout")
    }
}

// TODO: support keypress events
pub enum GeneratedTextInputEvent {
    Input,
    Select,
    PreEditChange,
    Submit,
}

pub struct TextInputData {
    /// A parley TextEditor instance
    pub editor: Box<parley::PlainEditor<TextBrush>>,
    /// Whether the input is a singleline or multiline input
    pub is_multiline: bool,
    /// The scroll offset of the text content within the input, in CSS (unscaled) pixels.
    ///
    /// For single-line inputs this is a horizontal offset; for multi-line inputs it is a
    /// vertical offset. It is kept up to date so that the caret remains visible within the
    /// input's content box.
    pub scroll_offset: f32,
}

// FIXME: Implement Clone for PlainEditor
impl Clone for TextInputData {
    fn clone(&self) -> Self {
        TextInputData::new(self.is_multiline)
    }
}

impl TextInputData {
    pub fn new(is_multiline: bool) -> Self {
        let editor = Box::new(parley::PlainEditor::new(16.0));
        Self {
            editor,
            is_multiline,
            scroll_offset: 0.0,
        }
    }

    pub fn set_text(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        text: &str,
    ) {
        if self.editor.text() != text {
            self.editor.set_text(text);
            self.editor.driver(font_ctx, layout_ctx).refresh_layout();
        }
    }

    /// Recompute [`Self::scroll_offset`] so that the caret stays visible within the input's
    /// content box.
    ///
    /// `content_box_width` and `content_box_height` are the dimensions of the input's content
    /// box in CSS (unscaled) pixels.
    pub fn clamp_scroll_offset(&mut self, content_box_width: f32, content_box_height: f32) {
        let Some(layout) = self.editor.try_layout() else {
            return;
        };
        // Parley lays out at the editor's scale, so its geometry is in scaled (device) pixels.
        // We convert into CSS (unscaled) pixels to match `scroll_offset` and the content box.
        let scale = layout.scale();

        // The caret geometry relative to the start of the text content.
        let Some(caret) = self.editor.cursor_geometry(1.5) else {
            return;
        };

        // Caret bounds and content/viewport extents along the scrolling axis (CSS pixels).
        let (caret_start, caret_end, content, viewport) = if self.is_multiline {
            (
                caret.y0 as f32 / scale,
                caret.y1 as f32 / scale,
                layout.height() / scale,
                content_box_height,
            )
        } else {
            (
                caret.x0 as f32 / scale,
                caret.x1 as f32 / scale,
                layout.full_width() / scale,
                content_box_width,
            )
        };

        let mut offset = self.scroll_offset;

        // Scroll so that both edges of the caret are within the visible region.
        if caret_end > offset + viewport {
            offset = caret_end - viewport;
        }
        if caret_start < offset {
            offset = caret_start;
        }

        // Never scroll past the content, and never scroll into negative space. The content
        // extent includes the caret so that a caret at the very end remains fully visible
        // (its rendered width extends slightly past the text).
        let max_offset = (content.max(caret_end) - viewport).max(0.0);
        self.scroll_offset = offset.clamp(0.0, max_offset);
    }

    /// The maximum valid value of [`Self::scroll_offset`] (in CSS pixels) given the input's
    /// content box, i.e. the extent by which the text content overflows the content box along
    /// the input's scroll axis.
    ///
    /// `content_box_width` and `content_box_height` are the dimensions of the input's content
    /// box in CSS (unscaled) pixels.
    pub fn max_scroll_offset(&self, content_box_width: f32, content_box_height: f32) -> f32 {
        let Some(layout) = self.editor.try_layout() else {
            return 0.0;
        };
        let scale = layout.scale();
        let (content, viewport) = if self.is_multiline {
            (layout.height() / scale, content_box_height)
        } else {
            (layout.full_width() / scale, content_box_width)
        };
        (content - viewport).max(0.0)
    }

    /// Scroll the input's text content by `delta` CSS pixels along its scroll axis (horizontal
    /// for single-line inputs, vertical for multi-line inputs), clamping to the scrollable
    /// range.
    ///
    /// Returns the portion of `delta` that could not be consumed (because the input was already
    /// scrolled to its limit), so the caller can bubble it up to an ancestor scroller.
    pub fn scroll_by(
        &mut self,
        delta: f32,
        content_box_width: f32,
        content_box_height: f32,
    ) -> f32 {
        let max_offset = self.max_scroll_offset(content_box_width, content_box_height);
        if max_offset <= 0.0 {
            return delta;
        }

        // Match the sign convention used for block scrolling: a positive delta decreases the
        // scroll offset.
        let new_offset = (self.scroll_offset - delta).clamp(0.0, max_offset);
        let consumed = self.scroll_offset - new_offset;
        self.scroll_offset = new_offset;
        delta - consumed
    }

    pub(crate) fn apply_keypress_event(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        shell_provider: &dyn ShellProvider,
        event: BlitzKeyEvent,
    ) -> Option<GeneratedTextInputEvent> {
        // Do nothing if it is a keyup event
        if !event.state.is_pressed() {
            return None;
        }

        let mods = event.modifiers;
        let shift = mods.contains(Modifiers::SHIFT);
        let action_mod = mods.contains(ACTION_MOD);

        let is_multiline = self.is_multiline;
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);
        match event.key {
            Key::Character(c) if action_mod && matches!(c.as_str(), "c" | "x" | "v") => {
                match c.to_lowercase().as_str() {
                    "c" => {
                        if let Some(text) = driver.editor.selected_text() {
                            let _ = shell_provider.set_clipboard_text(text.to_owned());
                        }
                    }
                    "x" => {
                        if let Some(text) = driver.editor.selected_text() {
                            let _ = shell_provider.set_clipboard_text(text.to_owned());
                            driver.delete_selection()
                        }
                    }
                    "v" => {
                        let text = shell_provider.get_clipboard_text().unwrap_or_default();
                        driver.insert_or_replace_selection(&text)
                    }
                    _ => unreachable!(),
                }

                return Some(GeneratedTextInputEvent::Input);
            }
            Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
                if shift {
                    driver.collapse_selection()
                } else {
                    driver.select_all()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::ArrowLeft => {
                if action_mod {
                    if shift {
                        driver.select_word_left()
                    } else {
                        driver.move_word_left()
                    }
                } else if shift {
                    driver.select_left()
                } else {
                    driver.move_left()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::ArrowRight => {
                if action_mod {
                    if shift {
                        driver.select_word_right()
                    } else {
                        driver.move_word_right()
                    }
                } else if shift {
                    driver.select_right()
                } else {
                    driver.move_right()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::ArrowUp => {
                if shift {
                    driver.select_up()
                } else {
                    driver.move_up()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::ArrowDown => {
                if shift {
                    driver.select_down()
                } else {
                    driver.move_down()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Home => {
                if action_mod {
                    if shift {
                        driver.select_to_text_start()
                    } else {
                        driver.move_to_text_start()
                    }
                } else if shift {
                    driver.select_to_line_start()
                } else {
                    driver.move_to_line_start()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::End => {
                if action_mod {
                    if shift {
                        driver.select_to_text_end()
                    } else {
                        driver.move_to_text_end()
                    }
                } else if shift {
                    driver.select_to_line_end()
                } else {
                    driver.move_to_line_end()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Delete => {
                if action_mod {
                    driver.delete_word()
                } else {
                    driver.delete()
                }
                return Some(GeneratedTextInputEvent::Input);
            }

            // On macOS this is handled by the apple standard keybindings
            #[cfg(not(target_os = "macos"))]
            Key::Backspace => {
                if action_mod {
                    driver.backdelete_word()
                } else {
                    driver.backdelete()
                }
                return Some(GeneratedTextInputEvent::Input);
            }

            Key::Character(c) if c == "\n" => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            Key::Enter => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            Key::Character(s)
                if !mods.contains(Modifiers::CONTROL) && !mods.contains(Modifiers::SUPER) =>
            {
                driver.insert_or_replace_selection(&s);
                return Some(GeneratedTextInputEvent::Input);
            }
            _ => {}
        };

        None
    }

    pub(crate) fn apply_apple_standard_keybinding(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        shell_provider: &dyn ShellProvider,
        command: &str,
    ) -> Option<GeneratedTextInputEvent> {
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);
        let is_multiline = self.is_multiline;

        match command {
            // Inserting Content

            // Inserts a backtab character.
            "insertBacktab:" => {}
            // Inserts a container break, such as a new page break.
            "insertContainerBreak:" => {}
            // Inserts a double quotation mark without substituting a curly quotation mark.
            "insertDoubleQuoteIgnoringSubstitution:" => {
                driver.insert_or_replace_selection("\"");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a line break character.
            "insertLineBreak:" => {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a newline character.
            "insertNewline:" => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            // Inserts a newline character without invoking the field editor’s normal handling to end editing.
            "insertNewlineIgnoringFieldEditor:" => {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a paragraph separator.
            "insertParagraphSeparator:" => {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedTextInputEvent::Input);
            }
            "insertSingleQuoteIgnoringSubstitution:" => {
                driver.insert_or_replace_selection("'");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a tab character.
            "insertTab:" | "insertTabIgnoringFieldEditor:" => {
                // Ignore for now seeing as parley has poor support for laying out tabs
            }
            // Inserts the text you specify.
            "insertText:" => {}

            // Deleting Content

            // Deletes content moving backward from the current insertion point.
            // TODO: handle deleteBackwardByDecomposingPreviousCharacter separately
            "deleteBackward:" | "deleteBackwardByDecomposingPreviousCharacter:" => {
                driver.backdelete();
                return Some(GeneratedTextInputEvent::Input);
            }
            "deleteForward:" => {
                driver.delete();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the beginning of the current line.
            "deleteToBeginningOfLine:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_line_start();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the beginning of the current paragraph.
            "deleteToEndOfLine:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_line_end();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }
            "deleteToBeginningOfParagraph:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_hard_line_start();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }

            // Deletes content from the insertion point to the end of the current line.
            "deleteToEndOfParagraph:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_hard_line_end();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the end of the current paragraph.
            "deleteWordBackward:" => {
                driver.backdelete_word();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes the word preceding the current insertion point.
            "deleteWordForward:" => {
                driver.delete_word();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes the current selection, placing it in a temporary buffer, such as the Clipboard.
            "yank:" => {
                if let Some(text) = driver.editor.selected_text() {
                    let _ = shell_provider.set_clipboard_text(text.to_owned());
                    driver.delete_selection();
                    return Some(GeneratedTextInputEvent::Input);
                }
            }

            // Moving the Insertion Pointer

            // Moves the insertion pointer backward in the current content.
            "moveBackward:" => {
                driver.move_left(); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moves the insertion pointer down in the current content.
            "moveDown:" => {
                driver.move_down();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer forward in the current content.
            "moveForward:" => {
                driver.move_right();
                return Some(GeneratedTextInputEvent::Select);
            } // TODO: Bidi-aware

            // Moves the insertion pointer left in the current content.
            "moveLeft:" => {
                driver.move_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer right in the current content.
            "moveRight:" => {
                driver.move_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer up in the current content.
            "moveUp:" => {
                driver.move_up();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Modifying the Selection

            // Extends the selection to include the content before the current selection.
            "moveBackwardAndModifySelection:" => {
                driver.select_left(); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content below the current selection.
            "moveDownAndModifySelection:" => {
                driver.select_down();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content after the current selection.
            "moveForwardAndModifySelection:" => {
                driver.select_right(); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content to the left of the current selection.
            "moveLeftAndModifySelection:" => {
                driver.select_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content to the right of the current selection.
            "moveRightAndModifySelection:" => {
                driver.select_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content above the current selection.
            "moveUpAndModifySelection:" => {
                driver.select_up();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Changing the Selection
            "selectAll:" => {
                driver.select_all();
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectLine:" => {
                driver.move_to_line_start();
                driver.select_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectParagraph:" => {
                driver.move_to_hard_line_start();
                driver.select_to_hard_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectWord:" => {
                // TODO
            }

            // Moving the Selection in Documents
            "moveToBeginningOfDocument:" => {
                driver.move_to_text_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfDocumentAndModifySelection:" => {
                driver.select_to_text_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfDocument:" => {
                driver.move_to_text_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfDocumentAndModifySelection:" => {
                driver.move_to_text_end();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection in Paragraphs
            "moveParagraphBackwardAndModifySelection:" => {}
            "moveParagraphForwardAndModifySelection:" => {}
            "moveToBeginningOfParagraph:" => {
                driver.move_to_hard_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfParagraphAndModifySelection:" => {
                driver.select_to_hard_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfParagraph:" => {
                driver.move_to_hard_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfParagraphAndModifySelection:" => {
                driver.select_to_hard_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection in Lines of Text
            "moveToBeginningOfLine:" => {
                driver.move_to_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfLineAndModifySelection:" => {
                driver.select_to_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfLine:" => {
                driver.move_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfLineAndModifySelection:" => {
                driver.select_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToLeftEndOfLine:" => {
                driver.move_to_text_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToLeftEndOfLineAndModifySelection:" => {
                driver.select_to_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToRightEndOfLine:" => {
                driver.move_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToRightEndOfLineAndModifySelection:" => {
                driver.select_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection by Word Boundaries
            "moveWordBackward:" => {
                driver.move_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordBackwardAndModifySelection:" => {
                driver.select_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordForward:" => {
                driver.move_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordForwardAndModifySelection:" => {
                driver.select_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordLeft:" => {
                driver.move_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordLeftAndModifySelection:" => {
                driver.select_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordRight:" => {
                driver.move_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordRightAndModifySelection:" => {
                driver.select_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Scrolling Content

            // Scrolls the content down by a page.
            "scrollPageDown:" => {}
            // Scrolls the content up by a page.
            "scrollPageUp:" => {}
            // Scrolls the content down by a line.
            "scrollLineDown:" => {}
            // Scrolls the content up by a line.
            "scrollLineUp:" => {}
            // Scrolls the content to the beginning of the document.
            "scrollToBeginningOfDocument:" => {}
            // Scrolls the content to the end of the document.
            "scrollToEndOfDocument:" => {}
            // Moves the visible content region down by a page.
            "pageDown:" => {}
            // Moves the visible content region up by a page.
            "pageUp:" => {}
            // Moves the visible content region down by a page, and extends the current selection.
            "pageDownAndModifySelection:" => {}
            // Moves the visible content region up by a page, and extends the current selection.
            "pageUpAndModifySelection:" => {}
            // Moves the visible content region so the current selection is visually centered.
            "centerSelectionInVisibleArea:" => {}

            // Transposing Elements

            // Transposes the content around the current selection.
            "transpose:" => {}
            // Transposes the words around the current selection.
            "transposeWords:" => {}

            // Indenting Content
            // Indents the content at the current selection.
            "indent:" => {}

            // Canceling Operations
            // Cancels the current operation.
            "cancelOperation:" => {}

            // Supporting QuickLook
            // Invokes QuickLook to preview the current selection.
            "quickLookPreviewItems:" => {}

            // Supporting Writing Directions
            "makeBaseWritingDirectionLeftToRight:" => {}
            "makeBaseWritingDirectionNatural:" => {}
            "makeBaseWritingDirectionRightToLeft:" => {}
            "makeTextWritingDirectionLeftToRight:" => {}
            "makeTextWritingDirectionNatural:" => {}
            "makeTextWritingDirectionRightToLeft:" => {}

            // Changing Capitalization
            "capitalizeWord:" => {}
            "changeCaseOfLetter:" => {}
            "lowercaseWord:" => {}
            "uppercaseWord:" => {}

            // Supporting Marked Selections
            "setMark:" => {}
            "selectToMark:" => {}
            "deleteToMark:" => {}
            "swapWithMark:" => {}

            // Supporting Autocomplete
            "complete:" => {}

            // Instance Methods
            "showContextMenuForSelection:" => {}

            // Unknown command
            _ => {}
        };

        None
    }

    pub(crate) fn apply_ime_event(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        event: BlitzImeEvent,
    ) -> Option<GeneratedTextInputEvent> {
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);

        match event {
            BlitzImeEvent::Enabled => {
                // Do nothing
                None
            }
            BlitzImeEvent::Disabled => {
                driver.clear_compose();
                Some(GeneratedTextInputEvent::PreEditChange)
            }
            BlitzImeEvent::Commit(text) => {
                driver.insert_or_replace_selection(&text);
                Some(GeneratedTextInputEvent::Input)
            }
            BlitzImeEvent::Preedit(text, cursor) => {
                if text.is_empty() {
                    driver.clear_compose();
                } else {
                    driver.set_compose(&text, cursor);
                }
                Some(GeneratedTextInputEvent::PreEditChange)
            }
            BlitzImeEvent::DeleteSurrounding {
                before_bytes,
                after_bytes,
            } => {
                let _ = before_bytes;
                let _ = after_bytes;
                // TODO
                None
            }
        }
    }
}
