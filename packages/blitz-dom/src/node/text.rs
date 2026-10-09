use blitz_traits::{
    events::{BlitzImeEvent, BlitzKeyEvent},
    node_id::NodeId,
    shell::ShellProvider,
};
use keyboard_types::{Key, Modifiers};

use crate::text::{
    Edit, EditEngine as _, EditableText as _, EditorMetrics, Motion, TextContext, TextEditor,
    TextInputDriver,
};
use crate::util::ACTION_MOD;

pub use crate::text::TextLayout;

/// A position in an inline root's laid-out text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineTextHit {
    pub node_id: NodeId,
    pub byte_offset: usize,
}

/// A piece of an inline root's laid-out content, in logical order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineContent {
    /// Text of node `node_id`: the bytes `range` of the layout text. The node is the text node,
    /// or the element it is in, as the text backend maps the text.
    Text {
        node_id: NodeId,
        range: std::ops::Range<usize>,
    },
    /// An inline box.
    Box(NodeId),
}

// TODO: support keypress events
pub enum GeneratedTextInputEvent {
    Input,
    Select,
    PreEditChange,
    Submit,
}

pub struct TextInputData {
    /// The text backend's editor
    pub editor: Box<TextEditor>,
    /// Whether the input is a singleline or multiline input
    pub is_multiline: bool,
    /// Whether the input is `<input type=password>`, whose text is never copied and whose word
    /// motions and double clicks take the whole text, as in Chrome.
    pub is_password: bool,
    /// The scroll offset of the text content within the input, in CSS (unscaled) pixels.
    ///
    /// For single-line inputs this is a horizontal offset; for multi-line inputs it is a
    /// vertical offset. It is kept up to date so that the caret remains visible within the
    /// input's content box.
    pub scroll_offset: f32,
}

// FIXME: Implement Clone for the editor
impl Clone for TextInputData {
    fn clone(&self) -> Self {
        TextInputData::new(self.is_multiline)
    }
}

impl TextInputData {
    pub fn new(is_multiline: bool) -> Self {
        let editor = Box::new(<TextEditor as crate::text::EditEngine>::new(is_multiline));
        Self {
            editor,
            is_multiline,
            is_password: false,
            scroll_offset: 0.0,
        }
    }

    /// Returns `motion` as the input takes it: in a password field, Chrome moves a word to the
    /// text's start or end, since it shows no words.
    pub(crate) fn motion(&self, motion: Motion) -> Motion {
        match motion {
            Motion::WordLeft if self.is_password => Motion::TextStart,
            Motion::WordRight if self.is_password => Motion::TextEnd,
            motion => motion,
        }
    }

    /// Returns `edit` as the input takes it: in a password field, Chrome selects the whole text
    /// at a double or triple click, and moves by words as [`motion`](Self::motion) says.
    pub(crate) fn edit<'e>(&self, edit: Edit<'e>) -> Edit<'e> {
        if !self.is_password {
            return edit;
        }
        match edit {
            Edit::SelectWordAtPoint(..) | Edit::SelectHardLineAtPoint(..) => Edit::SelectAll,
            Edit::Move(motion) => Edit::Move(self.motion(motion)),
            Edit::Extend(motion) => Edit::Extend(self.motion(motion)),
            edit => edit,
        }
    }

    /// Sets the text, and lays it out again where it changed.
    pub(crate) fn set_text(&mut self, cx: &mut TextContext, text: &str) {
        self.editor.set_text(text);
        self.editor.refresh(cx);
    }

    /// Recompute [`Self::scroll_offset`] so that the caret stays visible within the input's
    /// content box.
    ///
    /// `content_box_width` and `content_box_height` are the dimensions of the input's content
    /// box in CSS (unscaled) pixels.
    pub fn clamp_scroll_offset(&mut self, content_box_width: f32, content_box_height: f32) {
        // The editor lays out at its scale, so its geometry is in scaled (device) pixels.
        // We convert into CSS (unscaled) pixels to match `scroll_offset` and the content box.
        let Some(EditorMetrics { size, scale }) = self.editor.metrics() else {
            return;
        };

        // The caret geometry relative to the start of the text content.
        let Some(caret) = self.editor.caret_rect() else {
            return;
        };

        // Caret bounds and content/viewport extents along the scrolling axis (CSS pixels).
        let (caret_start, caret_end, content, viewport) = if self.is_multiline {
            (
                caret.y0 as f32 / scale,
                caret.y1 as f32 / scale,
                size.height as f32 / scale,
                content_box_height,
            )
        } else {
            (
                caret.x0 as f32 / scale,
                caret.x1 as f32 / scale,
                size.width as f32 / scale,
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
        let Some(EditorMetrics { size, scale }) = self.editor.metrics() else {
            return 0.0;
        };
        let (content, viewport) = if self.is_multiline {
            (size.height as f32 / scale, content_box_height)
        } else {
            (size.width as f32 / scale, content_box_width)
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

    /// Applies a key press. Where `reveals`, a password field shows the character typed in the
    /// clear.
    pub(crate) fn apply_keypress_event(
        &mut self,
        cx: &mut TextContext,
        shell_provider: &dyn ShellProvider,
        event: BlitzKeyEvent,
        reveals: bool,
    ) -> Option<GeneratedTextInputEvent> {
        // Do nothing if it is a keyup event
        if !event.state.is_pressed() {
            return None;
        }

        let mods = event.modifiers;
        let shift = mods.contains(Modifiers::SHIFT);
        let action_mod = mods.contains(ACTION_MOD);

        let is_multiline = self.is_multiline;
        let is_password = self.is_password;
        let word_left = self.motion(Motion::WordLeft);
        let word_right = self.motion(Motion::WordRight);
        let reveals = reveals && self.is_password;
        let mut driver = TextInputDriver {
            editor: &mut self.editor,
            cx,
        };
        match event.key {
            // Chrome neither copies nor cuts a password field's text, and fires no event for it.
            Key::Character(c) if action_mod && is_password && matches!(c.as_str(), "c" | "x") => {
                return None;
            }
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
                            driver.edit(Edit::DeleteSelection)
                        }
                    }
                    "v" => {
                        let text = shell_provider.get_clipboard_text().unwrap_or_default();
                        driver.edit(Edit::Insert(&text))
                    }
                    _ => unreachable!(),
                }

                return Some(GeneratedTextInputEvent::Input);
            }
            Key::Character(c)
                if action_mod
                    && (c.eq_ignore_ascii_case("z")
                        || (cfg!(not(target_os = "macos")) && c.eq_ignore_ascii_case("y"))) =>
            {
                let redo = shift || c.eq_ignore_ascii_case("y");
                return undo_or_redo(&mut driver, if redo { Edit::Redo } else { Edit::Undo });
            }
            Key::Undo => return undo_or_redo(&mut driver, Edit::Undo),
            Key::Redo => return undo_or_redo(&mut driver, Edit::Redo),
            Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
                if shift {
                    driver.edit(Edit::CollapseSelection)
                } else {
                    driver.edit(Edit::SelectAll)
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::ArrowLeft => {
                if action_mod {
                    if shift {
                        driver.edit(Edit::Extend(word_left))
                    } else {
                        driver.edit(Edit::Move(word_left))
                    }
                } else if shift {
                    driver.edit(Edit::Extend(Motion::Left))
                } else {
                    driver.edit(Edit::Move(Motion::Left))
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::ArrowRight => {
                if action_mod {
                    if shift {
                        driver.edit(Edit::Extend(word_right))
                    } else {
                        driver.edit(Edit::Move(word_right))
                    }
                } else if shift {
                    driver.edit(Edit::Extend(Motion::Right))
                } else {
                    driver.edit(Edit::Move(Motion::Right))
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::ArrowUp => {
                if shift {
                    driver.edit(Edit::Extend(Motion::Up))
                } else {
                    driver.edit(Edit::Move(Motion::Up))
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::ArrowDown => {
                if shift {
                    driver.edit(Edit::Extend(Motion::Down))
                } else {
                    driver.edit(Edit::Move(Motion::Down))
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Home => {
                if action_mod {
                    if shift {
                        driver.edit(Edit::Extend(Motion::TextStart))
                    } else {
                        driver.edit(Edit::Move(Motion::TextStart))
                    }
                } else if shift {
                    driver.edit(Edit::Extend(Motion::LineStart))
                } else {
                    driver.edit(Edit::Move(Motion::LineStart))
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::End => {
                if action_mod {
                    if shift {
                        driver.edit(Edit::Extend(Motion::TextEnd))
                    } else {
                        driver.edit(Edit::Move(Motion::TextEnd))
                    }
                } else if shift {
                    driver.edit(Edit::Extend(Motion::LineEnd))
                } else {
                    driver.edit(Edit::Move(Motion::LineEnd))
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Delete => {
                if action_mod {
                    delete_word(&mut driver, is_password, true)
                } else {
                    driver.edit(Edit::Delete)
                }
                return Some(GeneratedTextInputEvent::Input);
            }

            // On macOS this is handled by the apple standard keybindings
            #[cfg(not(target_os = "macos"))]
            Key::Backspace => {
                if action_mod {
                    delete_word(&mut driver, is_password, false)
                } else {
                    driver.edit(Edit::Backdelete)
                }
                return Some(GeneratedTextInputEvent::Input);
            }

            Key::Character(c) if c == "\n" => {
                if is_multiline {
                    driver.edit(Edit::Insert("\n"));
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            Key::Enter => {
                if is_multiline {
                    driver.edit(Edit::Insert("\n"));
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            Key::Character(s)
                if !mods.contains(Modifiers::CONTROL) && !mods.contains(Modifiers::SUPER) =>
            {
                driver.edit(insert(&s, reveals));
                return Some(GeneratedTextInputEvent::Input);
            }
            _ => {}
        };

        None
    }

    pub(crate) fn apply_apple_standard_keybinding(
        &mut self,
        cx: &mut TextContext,
        shell_provider: &dyn ShellProvider,
        command: &str,
    ) -> Option<GeneratedTextInputEvent> {
        let is_multiline = self.is_multiline;
        let is_password = self.is_password;
        let word_left = self.motion(Motion::WordLeft);
        let word_right = self.motion(Motion::WordRight);
        let mut driver = TextInputDriver {
            editor: &mut self.editor,
            cx,
        };

        match command {
            // Inserting Content

            // Inserts a backtab character.
            "insertBacktab:" => {}
            // Inserts a container break, such as a new page break.
            "insertContainerBreak:" => {}
            // Inserts a double quotation mark without substituting a curly quotation mark.
            "insertDoubleQuoteIgnoringSubstitution:" => {
                driver.edit(Edit::Insert("\""));
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a line break character.
            "insertLineBreak:" => {
                driver.edit(Edit::Insert("\n"));
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a newline character.
            "insertNewline:" => {
                if is_multiline {
                    driver.edit(Edit::Insert("\n"));
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            // Inserts a newline character without invoking the field editor’s normal handling to end editing.
            "insertNewlineIgnoringFieldEditor:" => {
                driver.edit(Edit::Insert("\n"));
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a paragraph separator.
            "insertParagraphSeparator:" => {
                driver.edit(Edit::Insert("\n"));
                return Some(GeneratedTextInputEvent::Input);
            }
            "insertSingleQuoteIgnoringSubstitution:" => {
                driver.edit(Edit::Insert("'"));
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
                driver.edit(Edit::Backdelete);
                return Some(GeneratedTextInputEvent::Input);
            }
            "deleteForward:" => {
                driver.edit(Edit::Delete);
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the beginning of the current line.
            "deleteToBeginningOfLine:" => {
                if driver.editor.is_selection_collapsed() {
                    driver.edit(Edit::Extend(Motion::LineStart));
                }
                driver.edit(Edit::DeleteSelection);
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the beginning of the current paragraph.
            "deleteToEndOfLine:" => {
                if driver.editor.is_selection_collapsed() {
                    driver.edit(Edit::Extend(Motion::LineEnd));
                }
                driver.edit(Edit::DeleteSelection);
                return Some(GeneratedTextInputEvent::Input);
            }
            "deleteToBeginningOfParagraph:" => {
                if driver.editor.is_selection_collapsed() {
                    driver.edit(Edit::Extend(Motion::HardLineStart));
                }
                driver.edit(Edit::DeleteSelection);
                return Some(GeneratedTextInputEvent::Input);
            }

            // Deletes content from the insertion point to the end of the current line.
            "deleteToEndOfParagraph:" => {
                if driver.editor.is_selection_collapsed() {
                    driver.edit(Edit::Extend(Motion::HardLineEnd));
                }
                driver.edit(Edit::DeleteSelection);
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the end of the current paragraph.
            "deleteWordBackward:" => {
                delete_word(&mut driver, is_password, false);
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes the word preceding the current insertion point.
            "deleteWordForward:" => {
                delete_word(&mut driver, is_password, true);
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes the current selection, placing it in a temporary buffer, such as the Clipboard.
            "yank:" => {
                if let Some(text) = driver.editor.selected_text().filter(|_| !is_password) {
                    let _ = shell_provider.set_clipboard_text(text.to_owned());
                    driver.edit(Edit::DeleteSelection);
                    return Some(GeneratedTextInputEvent::Input);
                }
            }

            // Moving the Insertion Pointer

            // Moves the insertion pointer backward in the current content.
            "moveBackward:" => {
                driver.edit(Edit::Move(Motion::Left)); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moves the insertion pointer down in the current content.
            "moveDown:" => {
                driver.edit(Edit::Move(Motion::Down));
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer forward in the current content.
            "moveForward:" => {
                driver.edit(Edit::Move(Motion::Right));
                return Some(GeneratedTextInputEvent::Select);
            } // TODO: Bidi-aware

            // Moves the insertion pointer left in the current content.
            "moveLeft:" => {
                driver.edit(Edit::Move(Motion::Left));
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer right in the current content.
            "moveRight:" => {
                driver.edit(Edit::Move(Motion::Right));
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer up in the current content.
            "moveUp:" => {
                driver.edit(Edit::Move(Motion::Up));
                return Some(GeneratedTextInputEvent::Select);
            }

            // Modifying the Selection

            // Extends the selection to include the content before the current selection.
            "moveBackwardAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::Left)); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content below the current selection.
            "moveDownAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::Down));
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content after the current selection.
            "moveForwardAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::Right)); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content to the left of the current selection.
            "moveLeftAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::Left));
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content to the right of the current selection.
            "moveRightAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::Right));
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content above the current selection.
            "moveUpAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::Up));
                return Some(GeneratedTextInputEvent::Select);
            }

            // Changing the Selection
            "selectAll:" => {
                driver.edit(Edit::SelectAll);
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectLine:" => {
                driver.edit(Edit::Select(Motion::LineStart, Motion::LineEnd));
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectParagraph:" => {
                driver.edit(Edit::Select(Motion::HardLineStart, Motion::HardLineEnd));
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectWord:" => {
                // TODO
            }

            // Moving the Selection in Documents
            "moveToBeginningOfDocument:" => {
                driver.edit(Edit::Move(Motion::TextStart));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfDocumentAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::TextStart));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfDocument:" => {
                driver.edit(Edit::Move(Motion::TextEnd));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfDocumentAndModifySelection:" => {
                driver.edit(Edit::Move(Motion::TextEnd));
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection in Paragraphs
            "moveParagraphBackwardAndModifySelection:" => {}
            "moveParagraphForwardAndModifySelection:" => {}
            "moveToBeginningOfParagraph:" => {
                driver.edit(Edit::Move(Motion::HardLineStart));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfParagraphAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::HardLineStart));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfParagraph:" => {
                driver.edit(Edit::Move(Motion::HardLineEnd));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfParagraphAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::HardLineEnd));
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection in Lines of Text
            "moveToBeginningOfLine:" => {
                driver.edit(Edit::Move(Motion::LineStart));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfLineAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::LineStart));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfLine:" => {
                driver.edit(Edit::Move(Motion::LineEnd));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfLineAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::LineEnd));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToLeftEndOfLine:" => {
                driver.edit(Edit::Move(Motion::TextStart));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToLeftEndOfLineAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::LineStart));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToRightEndOfLine:" => {
                driver.edit(Edit::Move(Motion::LineEnd));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToRightEndOfLineAndModifySelection:" => {
                driver.edit(Edit::Extend(Motion::LineEnd));
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection by Word Boundaries
            "moveWordBackward:" => {
                driver.edit(Edit::Move(word_left));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordBackwardAndModifySelection:" => {
                driver.edit(Edit::Extend(word_left));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordForward:" => {
                driver.edit(Edit::Move(word_right));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordForwardAndModifySelection:" => {
                driver.edit(Edit::Extend(word_right));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordLeft:" => {
                driver.edit(Edit::Move(word_left));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordLeftAndModifySelection:" => {
                driver.edit(Edit::Extend(word_left));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordRight:" => {
                driver.edit(Edit::Move(word_right));
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordRightAndModifySelection:" => {
                driver.edit(Edit::Extend(word_right));
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

    /// Applies an input method's event. Where `reveals`, a password field shows a committed
    /// character in the clear.
    pub(crate) fn apply_ime_event(
        &mut self,
        cx: &mut TextContext,
        event: BlitzImeEvent,
        reveals: bool,
    ) -> Option<GeneratedTextInputEvent> {
        let reveals = reveals && self.is_password;
        let mut driver = TextInputDriver {
            editor: &mut self.editor,
            cx,
        };

        match event {
            BlitzImeEvent::Enabled => {
                // Do nothing
                None
            }
            BlitzImeEvent::Disabled => {
                driver.edit(Edit::ClearCompose);
                Some(GeneratedTextInputEvent::PreEditChange)
            }
            BlitzImeEvent::Commit(text) => {
                driver.edit(insert(&text, reveals));
                Some(GeneratedTextInputEvent::Input)
            }
            BlitzImeEvent::Preedit(text, cursor) => {
                if text.is_empty() {
                    driver.edit(Edit::ClearCompose);
                } else {
                    driver.edit(Edit::SetCompose(&text, cursor));
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

/// Returns the edit that inserts `text`, which a password field that `reveals` what is typed
/// shows in the clear where it is one grapheme.
fn insert(text: &str, reveals: bool) -> Edit<'_> {
    if reveals && is_one_grapheme(text) {
        Edit::InsertRevealed(text)
    } else {
        Edit::Insert(text)
    }
}

/// Returns whether `text` is one grapheme.
fn is_one_grapheme(text: &str) -> bool {
    // The segmenter gives the text's start, then the end of each grapheme.
    icu_segmenter::GraphemeClusterSegmenter::new()
        .segment_str(text)
        .count()
        == 2
}

/// Deletes the selection or the word before or after the caret; in a password field, which shows
/// no words, the text before or after it, as in Chrome.
fn delete_word(driver: &mut TextInputDriver<'_>, is_password: bool, forward: bool) {
    if !is_password {
        driver.edit(if forward {
            Edit::DeleteWord
        } else {
            Edit::BackdeleteWord
        });
        return;
    }
    if driver.editor.is_selection_collapsed() {
        driver.edit(Edit::Extend(if forward {
            Motion::TextEnd
        } else {
            Motion::TextStart
        }));
    }
    driver.edit(Edit::DeleteSelection);
}

/// Applies an [`Edit::Undo`] or an [`Edit::Redo`], which changes the text only where the text
/// backend keeps a history.
fn undo_or_redo(
    driver: &mut TextInputDriver<'_>,
    edit: Edit<'_>,
) -> Option<GeneratedTextInputEvent> {
    let before = driver.raw_text().to_owned();
    driver.edit(edit);
    Some(if driver.raw_text() == before {
        GeneratedTextInputEvent::Select
    } else {
        GeneratedTextInputEvent::Input
    })
}
