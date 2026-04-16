use crate::{
    BaseDocument,
    node::{TextBrush, TextInputData},
};
use blitz_traits::{
    SmolStr,
    events::{BlitzInputEvent, BlitzKeyEvent, DomEvent, DomEventData},
    shell::ShellProvider,
};
use keyboard_types::{Key, Modifiers};
use markup5ever::local_name;
use parley::{FontContext, LayoutContext};

// TODO: support keypress events
enum GeneratedEvent {
    Input,
    Select,
    Submit,
}

pub(super) enum KeyboardOrTextInputEvent {
    KeyPress(BlitzKeyEvent),
    AppleStandardKeyBinding(SmolStr),
}

pub(crate) fn handle_key_or_input_event<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target: usize,
    event: KeyboardOrTextInputEvent,
    mut dispatch_event: F,
) {
    if let KeyboardOrTextInputEvent::KeyPress(event) = &event {
        if event.key == Key::Tab {
            doc.focus_next_node();
            return;
        }

        // Handle copy (Ctrl+C/Cmd+C) for text selection when no text input is focused
        if event.state.is_pressed() {
            let action_mod = event.modifiers.contains(ACTION_MOD);
            if action_mod {
                if let Key::Character(c) = &event.key {
                    if c.to_lowercase() == "c" {
                        // Check if we have a text selection (and no focused text input)
                        let has_focused_text_input = doc.focus_node_id.is_some_and(|id| {
                            doc.get_node(id)
                                .and_then(|n| n.element_data())
                                .is_some_and(|e| e.text_input_data().is_some())
                        });

                        if !has_focused_text_input {
                            if let Some(text) = doc.get_selected_text() {
                                let _ = doc.shell_provider.set_clipboard_text(text);
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(node_id) = doc.focus_node_id {
        if target != node_id {
            return;
        }

        let node = &mut doc.nodes[node_id];
        let Some(element_data) = node.element_data_mut() else {
            return;
        };

        if let Some(input_data) = element_data.text_input_data_mut() {
            let generated_event = match event {
                KeyboardOrTextInputEvent::KeyPress(blitz_key_event) => apply_keypress_event(
                    input_data,
                    &mut doc.font_ctx.lock().unwrap(),
                    &mut doc.layout_ctx,
                    &*doc.shell_provider,
                    blitz_key_event,
                ),
                KeyboardOrTextInputEvent::AppleStandardKeyBinding(command) => {
                    apply_apple_standard_keybinding(
                        input_data,
                        &mut doc.font_ctx.lock().unwrap(),
                        &mut doc.layout_ctx,
                        &*doc.shell_provider,
                        &command,
                    )
                }
            };

            if let Some(generated_event) = generated_event {
                match generated_event {
                    GeneratedEvent::Input => {
                        let value = input_data.editor.raw_text().to_string();
                        dispatch_event(DomEvent::new(
                            node_id,
                            DomEventData::Input(BlitzInputEvent { value }),
                        ));
                        doc.shell_provider.request_redraw();
                    }
                    GeneratedEvent::Select => {
                        doc.shell_provider.request_redraw();
                    }
                    GeneratedEvent::Submit => {
                        // TODO: Generate submit event that can be handled by script
                        implicit_form_submission(doc, target);
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
const ACTION_MOD: Modifiers = Modifiers::SUPER;
#[cfg(not(target_os = "macos"))]
const ACTION_MOD: Modifiers = Modifiers::CONTROL;

fn apply_keypress_event(
    input_data: &mut TextInputData,
    font_ctx: &mut FontContext,
    layout_ctx: &mut LayoutContext<TextBrush>,
    shell_provider: &dyn ShellProvider,
    event: BlitzKeyEvent,
) -> Option<GeneratedEvent> {
    // Do nothing if it is a keyup event
    if !event.state.is_pressed() {
        return None;
    }

    let mods = event.modifiers;
    let shift = mods.contains(Modifiers::SHIFT);
    let action_mod = mods.contains(ACTION_MOD);

    let is_multiline = input_data.is_multiline;
    let editor = &mut input_data.editor;
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

            return Some(GeneratedEvent::Input);
        }
        Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
            if shift {
                driver.collapse_selection()
            } else {
                driver.select_all()
            }
            return Some(GeneratedEvent::Select);
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
            return Some(GeneratedEvent::Select);
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
            return Some(GeneratedEvent::Select);
        }
        Key::ArrowUp => {
            if shift {
                driver.select_up()
            } else {
                driver.move_up()
            }
            return Some(GeneratedEvent::Select);
        }
        Key::ArrowDown => {
            if shift {
                driver.select_down()
            } else {
                driver.move_down()
            }
            return Some(GeneratedEvent::Select);
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
            return Some(GeneratedEvent::Select);
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
            return Some(GeneratedEvent::Select);
        }
        Key::Delete => {
            if action_mod {
                driver.delete_word()
            } else {
                driver.delete()
            }
            return Some(GeneratedEvent::Input);
        }

        // On macOS this is handled by the apple standard keybindings
        #[cfg(not(target_os = "macos"))]
        Key::Backspace => {
            if action_mod {
                driver.backdelete_word()
            } else {
                driver.backdelete()
            }
            return Some(GeneratedEvent::Input);
        }

        Key::Character(c) if c == "\n" => {
            if is_multiline {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedEvent::Input);
            } else {
                return Some(GeneratedEvent::Submit);
            }
        }
        Key::Enter => {
            if is_multiline {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedEvent::Input);
            } else {
                return Some(GeneratedEvent::Submit);
            }
        }
        Key::Character(s)
            if !mods.contains(Modifiers::CONTROL) && !mods.contains(Modifiers::SUPER) =>
        {
            driver.insert_or_replace_selection(&s);
            return Some(GeneratedEvent::Input);
        }
        _ => {}
    };

    None
}

fn apply_apple_standard_keybinding(
    input_data: &mut TextInputData,
    font_ctx: &mut FontContext,
    layout_ctx: &mut LayoutContext<TextBrush>,
    shell_provider: &dyn ShellProvider,
    command: &str,
) -> Option<GeneratedEvent> {
    let editor = &mut input_data.editor;
    let mut driver = editor.driver(font_ctx, layout_ctx);

    match command {
        // Inserting Content

        // Inserts a backtab character.
        "insertBacktab:" => {}
        // Inserts a container break, such as a new page break.
        "insertContainerBreak:" => {}
        // Inserts a double quotation mark without substituting a curly quotation mark.
        "insertDoubleQuoteIgnoringSubstitution:" => {
            driver.insert_or_replace_selection("\"");
            return Some(GeneratedEvent::Input);
        }
        // Inserts a line break character.
        "insertLineBreak:" => {
            driver.insert_or_replace_selection("\n");
            return Some(GeneratedEvent::Input);
        }
        // Inserts a newline character.
        "insertNewline:" => {
            driver.insert_or_replace_selection("\n");
            return Some(GeneratedEvent::Input);
        }
        // Inserts a newline character without invoking the field editor’s normal handling to end editing.
        "insertNewlineIgnoringFieldEditor:" => {
            driver.insert_or_replace_selection("\n");
            return Some(GeneratedEvent::Input);
        }
        // Inserts a paragraph separator.
        "insertParagraphSeparator:" => {
            driver.insert_or_replace_selection("\n");
            return Some(GeneratedEvent::Input);
        }
        "insertSingleQuoteIgnoringSubstitution:" => {
            driver.insert_or_replace_selection("'");
            return Some(GeneratedEvent::Input);
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
            return Some(GeneratedEvent::Input);
        }
        "deleteForward:" => {
            driver.delete();
            return Some(GeneratedEvent::Input);
        }
        // Deletes content from the insertion point to the beginning of the current line.
        "deleteToBeginningOfLine:" => {
            if driver.editor.raw_selection().is_collapsed() {
                driver.select_to_line_start();
            }
            driver.delete_selection();
            return Some(GeneratedEvent::Input);
        }
        // Deletes content from the insertion point to the beginning of the current paragraph.
        "deleteToEndOfLine:" => {
            if driver.editor.raw_selection().is_collapsed() {
                driver.select_to_line_end();
            }
            driver.delete_selection();
            return Some(GeneratedEvent::Input);
        }
        "deleteToBeginningOfParagraph:" => {
            if driver.editor.raw_selection().is_collapsed() {
                driver.select_to_hard_line_start();
            }
            driver.delete_selection();
            return Some(GeneratedEvent::Input);
        }

        // Deletes content from the insertion point to the end of the current line.
        "deleteToEndOfParagraph:" => {
            if driver.editor.raw_selection().is_collapsed() {
                driver.select_to_hard_line_end();
            }
            driver.delete_selection();
            return Some(GeneratedEvent::Input);
        }
        // Deletes content from the insertion point to the end of the current paragraph.
        "deleteWordBackward:" => {
            driver.backdelete_word();
            return Some(GeneratedEvent::Input);
        }
        // Deletes the word preceding the current insertion point.
        "deleteWordForward:" => {
            driver.delete_word();
            return Some(GeneratedEvent::Input);
        }
        // Deletes the current selection, placing it in a temporary buffer, such as the Clipboard.
        "yank:" => {
            if let Some(text) = driver.editor.selected_text() {
                let _ = shell_provider.set_clipboard_text(text.to_owned());
                driver.delete_selection();
                return Some(GeneratedEvent::Input);
            }
        }

        // Moving the Insertion Pointer

        // Moves the insertion pointer backward in the current content.
        "moveBackward:" => {
            driver.move_left(); // TODO: Bidi-aware
            return Some(GeneratedEvent::Select);
        }

        // Moves the insertion pointer down in the current content.
        "moveDown:" => {
            driver.move_down();
            return Some(GeneratedEvent::Select);
        }
        // Moves the insertion pointer forward in the current content.
        "moveForward:" => {
            driver.move_right();
            return Some(GeneratedEvent::Select);
        } // TODO: Bidi-aware

        // Moves the insertion pointer left in the current content.
        "moveLeft:" => {
            driver.move_left();
            return Some(GeneratedEvent::Select);
        }
        // Moves the insertion pointer right in the current content.
        "moveRight:" => {
            driver.move_right();
            return Some(GeneratedEvent::Select);
        }
        // Moves the insertion pointer up in the current content.
        "moveUp:" => {
            driver.move_up();
            return Some(GeneratedEvent::Select);
        }

        // Modifying the Selection

        // Extends the selection to include the content before the current selection.
        "moveBackwardAndModifySelection:" => {
            driver.select_left(); // TODO: Bidi-aware
            return Some(GeneratedEvent::Select);
        }
        // Extends the selection to include the content below the current selection.
        "moveDownAndModifySelection:" => {
            driver.select_down();
            return Some(GeneratedEvent::Select);
        }
        // Extends the selection to include the content after the current selection.
        "moveForwardAndModifySelection:" => {
            driver.select_right(); // TODO: Bidi-aware
            return Some(GeneratedEvent::Select);
        }
        // Extends the selection to include the content to the left of the current selection.
        "moveLeftAndModifySelection:" => {
            driver.select_left();
            return Some(GeneratedEvent::Select);
        }
        // Extends the selection to include the content to the right of the current selection.
        "moveRightAndModifySelection:" => {
            driver.select_right();
            return Some(GeneratedEvent::Select);
        }
        // Extends the selection to include the content above the current selection.
        "moveUpAndModifySelection:" => {
            driver.select_up();
            return Some(GeneratedEvent::Select);
        }

        // Changing the Selection
        "selectAll:" => {
            driver.select_all();
            return Some(GeneratedEvent::Select);
        }
        "selectLine:" => {
            driver.move_to_line_start();
            driver.select_to_line_end();
            return Some(GeneratedEvent::Select);
        }
        "selectParagraph:" => {
            driver.move_to_hard_line_start();
            driver.select_to_hard_line_end();
            return Some(GeneratedEvent::Select);
        }
        "selectWord:" => {
            // TODO
        }

        // Moving the Selection in Documents
        "moveToBeginningOfDocument:" => {
            driver.move_to_text_start();
            return Some(GeneratedEvent::Select);
        }
        "moveToBeginningOfDocumentAndModifySelection:" => {
            driver.select_to_text_start();
            return Some(GeneratedEvent::Select);
        }
        "moveToEndOfDocument:" => {
            driver.move_to_text_end();
            return Some(GeneratedEvent::Select);
        }
        "moveToEndOfDocumentAndModifySelection:" => {
            driver.move_to_text_end();
            return Some(GeneratedEvent::Select);
        }

        // Moving the Selection in Paragraphs
        "moveParagraphBackwardAndModifySelection:" => {}
        "moveParagraphForwardAndModifySelection:" => {}
        "moveToBeginningOfParagraph:" => {
            driver.move_to_hard_line_start();
            return Some(GeneratedEvent::Select);
        }
        "moveToBeginningOfParagraphAndModifySelection:" => {
            driver.select_to_hard_line_start();
            return Some(GeneratedEvent::Select);
        }
        "moveToEndOfParagraph:" => {
            driver.move_to_hard_line_end();
            return Some(GeneratedEvent::Select);
        }
        "moveToEndOfParagraphAndModifySelection:" => {
            driver.select_to_hard_line_end();
            return Some(GeneratedEvent::Select);
        }

        // Moving the Selection in Lines of Text
        "moveToBeginningOfLine:" => {
            driver.move_to_line_start();
            return Some(GeneratedEvent::Select);
        }
        "moveToBeginningOfLineAndModifySelection:" => {
            driver.select_to_line_start();
            return Some(GeneratedEvent::Select);
        }
        "moveToEndOfLine:" => {
            driver.move_to_line_end();
            return Some(GeneratedEvent::Select);
        }
        "moveToEndOfLineAndModifySelection:" => {
            driver.select_to_line_end();
            return Some(GeneratedEvent::Select);
        }
        "moveToLeftEndOfLine:" => {
            driver.move_to_text_start();
            return Some(GeneratedEvent::Select);
        }
        "moveToLeftEndOfLineAndModifySelection:" => {
            driver.select_to_line_start();
            return Some(GeneratedEvent::Select);
        }
        "moveToRightEndOfLine:" => {
            driver.move_to_line_end();
            return Some(GeneratedEvent::Select);
        }
        "moveToRightEndOfLineAndModifySelection:" => {
            driver.select_to_line_end();
            return Some(GeneratedEvent::Select);
        }

        // Moving the Selection by Word Boundaries
        "moveWordBackward:" => {
            driver.move_word_left();
            return Some(GeneratedEvent::Select);
        }
        "moveWordBackwardAndModifySelection:" => {
            driver.select_word_left();
            return Some(GeneratedEvent::Select);
        }
        "moveWordForward:" => {
            driver.move_word_right();
            return Some(GeneratedEvent::Select);
        }
        "moveWordForwardAndModifySelection:" => {
            driver.select_word_right();
            return Some(GeneratedEvent::Select);
        }
        "moveWordLeft:" => {
            driver.move_word_left();
            return Some(GeneratedEvent::Select);
        }
        "moveWordLeftAndModifySelection:" => {
            driver.select_word_left();
            return Some(GeneratedEvent::Select);
        }
        "moveWordRight:" => {
            driver.move_word_right();
            return Some(GeneratedEvent::Select);
        }
        "moveWordRightAndModifySelection:" => {
            driver.select_word_right();
            return Some(GeneratedEvent::Select);
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

/// https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#field-that-blocks-implicit-submission
fn implicit_form_submission(doc: &BaseDocument, text_target: usize) {
    let Some(form_owner_id) = doc.controls_to_form.get(&text_target) else {
        return;
    };
    if doc
        .controls_to_form
        .iter()
        .filter(|(_control_id, form_id)| *form_id == form_owner_id)
        .filter_map(|(control_id, _)| doc.nodes[*control_id].element_data())
        .filter(|element_data| {
            element_data.attr(local_name!("type")).is_some_and(|t| {
                matches!(
                    t,
                    "text"
                        | "search"
                        | "email"
                        | "url"
                        | "tel"
                        | "password"
                        | "date"
                        | "month"
                        | "week"
                        | "time"
                        | "datetime-local"
                        | "number"
                )
            })
        })
        .count()
        > 1
    {
        return;
    }

    doc.submit_form(*form_owner_id, *form_owner_id);
}
