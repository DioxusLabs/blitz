use crate::{
    BaseDocument,
    node::{TextBrush, TextInputData},
};
use blitz_traits::BlitzKeyEvent;
use keyboard_types::{Key, Modifiers};
use parley::{FontContext, LayoutContext};

pub(crate) fn handle_keypress(doc: &mut BaseDocument, target: usize, event: BlitzKeyEvent) {
    if let Some(node_id) = doc.focus_node_id {
        if target != node_id {
            return;
        }

        let node = &mut doc.nodes[node_id];
        let text_input_data = node
            .data
            .downcast_element_mut()
            .and_then(|el| el.text_input_data_mut());

        if let Some(input_data) = text_input_data {
            apply_keypress_event(input_data, &mut doc.font_ctx, &mut doc.layout_ctx, event);
        }
    }
}

#[cfg(target_os = "macos")]
const ACTION_MOD: Modifiers = Modifiers::SUPER;
#[cfg(not(target_os = "macos"))]
const ACTION_MOD: Modifiers = Modifiers::CONTROL;

pub(crate) fn apply_keypress_event(
    input_data: &mut TextInputData,
    font_ctx: &mut FontContext,
    layout_ctx: &mut LayoutContext<TextBrush>,
    event: BlitzKeyEvent,
) {
    // Do nothing if it is a keyup event
    if !event.state.is_pressed() {
        return;
    }

    let mods = event.modifiers;
    let shift = mods.contains(Modifiers::SHIFT);
    let action_mod = mods.contains(ACTION_MOD);

    let is_multiline = input_data.is_multiline;
    let editor = &mut input_data.editor;
    let mut driver = editor.driver(font_ctx, layout_ctx);
    match event.key {
        #[cfg(all(feature = "clipboard", not(target_os = "android")))]
        Key::Character(c) if action_mod && matches!(c.as_str(), "c" | "x" | "v") => {
            use arboard::Clipboard;

            match c.to_lowercase().as_str() {
                "c" => {
                    if let Some(text) = driver.editor.selected_text() {
                        let mut cb = Clipboard::new().unwrap();
                        cb.set_text(text.to_owned()).ok();
                    }
                }
                "x" => {
                    if let Some(text) = driver.editor.selected_text() {
                        let mut cb = Clipboard::new().unwrap();
                        cb.set_text(text.to_owned()).ok();
                        driver.delete_selection()
                    }
                }
                "v" => {
                    let mut cb = Clipboard::new().unwrap();
                    let text = cb.get_text().unwrap_or_default();
                    driver.insert_or_replace_selection(&text)
                }
                _ => unreachable!(),
            }
        }
        Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
            if shift {
                driver.collapse_selection()
            } else {
                driver.select_all()
            }
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
        }
        Key::ArrowUp => {
            if shift {
                driver.select_up()
            } else {
                driver.move_up()
            }
        }
        Key::ArrowDown => {
            if shift {
                driver.select_down()
            } else {
                driver.move_down()
            }
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
        }
        Key::Delete => {
            if action_mod {
                driver.delete_word()
            } else {
                driver.delete()
            }
        }
        Key::Backspace => {
            if action_mod {
                driver.backdelete_word()
            } else {
                driver.backdelete()
            }
        }
        Key::Enter => {
            if is_multiline {
                driver.insert_or_replace_selection("\n");
            }
        }
        Key::Character(s) => driver.insert_or_replace_selection(&s),
        _ => {}
    };
}
