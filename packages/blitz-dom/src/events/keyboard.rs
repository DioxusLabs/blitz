use crate::{
    BaseDocument,
    node::{TextBrush, TextInputData},
};
use blitz_traits::{
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

pub(crate) fn handle_keypress<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target: usize,
    event: BlitzKeyEvent,
    mut dispatch_event: F,
) {
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

    if let Some(node_id) = doc.focus_node_id {
        if target != node_id {
            return;
        }

        let node = &mut doc.nodes[node_id];
        let Some(element_data) = node.element_data_mut() else {
            return;
        };

        if let Some(input_data) = element_data.text_input_data_mut() {
            let generated_event = apply_keypress_event(
                input_data,
                &mut doc.font_ctx.lock().unwrap(),
                &mut doc.layout_ctx,
                &*doc.shell_provider,
                event,
            );

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
        Key::Backspace => {
            if action_mod {
                driver.backdelete_word()
            } else {
                driver.backdelete()
            }
            return Some(GeneratedEvent::Input);
        }
        Key::Enter => {
            if is_multiline {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedEvent::Input);
            } else {
                return Some(GeneratedEvent::Submit);
            }
        }
        Key::Character(s) => {
            driver.insert_or_replace_selection(&s);
            return Some(GeneratedEvent::Input);
        }
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
