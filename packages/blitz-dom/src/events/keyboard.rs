use crate::node::TextBrush;
use parley::{FontContext, LayoutContext, PlainEditor};
use winit::{
    event::{KeyEvent, Modifiers},
    keyboard::{Key, NamedKey},
};

pub(crate) fn apply_keypress_event(
    editor: &mut PlainEditor<TextBrush>,
    font_ctx: &mut FontContext,
    layout_ctx: &mut LayoutContext<TextBrush>,
    event: KeyEvent,
    mods: Modifiers,
) {
    // Do nothing if it is a keyup event
    if !event.state.is_pressed() {
        return;
    }

    let shift = mods.state().shift_key();
    let action_mod = {
        if cfg!(target_os = "macos") {
            mods.state().super_key()
        } else {
            mods.state().control_key()
        }
    };

    // Small macro to reduce boilerplate
    macro_rules! transact {
        ($op:expr) => {{
            editor.transact(font_ctx, layout_ctx, $op);
        }};
    }

    match event.logical_key {
        #[cfg(not(target_os = "android"))]
        Key::Character(c) if action_mod && matches!(c.as_str(), "c" | "x" | "v") => {
            use arboard::Clipboard;

            match c.to_lowercase().as_str() {
                "c" => {
                    if let Some(text) = editor.selected_text() {
                        let mut cb = Clipboard::new().unwrap();
                        cb.set_text(text.to_owned()).ok();
                    }
                }
                "x" => {
                    if let Some(text) = editor.selected_text() {
                        let mut cb = Clipboard::new().unwrap();
                        cb.set_text(text.to_owned()).ok();
                        transact!(|txn| txn.delete_selection())
                    }
                }
                "v" => {
                    let mut cb = Clipboard::new().unwrap();
                    let text = cb.get_text().unwrap_or_default();
                    transact!(|txn| txn.insert_or_replace_selection(&text))
                }
                _ => unreachable!(),
            }
        }
        Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
            if shift {
                transact!(|txn| txn.collapse_selection())
            } else {
                transact!(|txn| txn.select_all())
            }
        }
        Key::Named(NamedKey::ArrowLeft) => {
            if action_mod {
                if shift {
                    transact!(|txn| txn.select_word_left())
                } else {
                    transact!(|txn| txn.move_word_left())
                }
            } else if shift {
                transact!(|txn| txn.select_left())
            } else {
                transact!(|txn| txn.move_left())
            }
        }
        Key::Named(NamedKey::ArrowRight) => {
            if action_mod {
                if shift {
                    transact!(|txn| txn.select_word_right())
                } else {
                    transact!(|txn| txn.move_word_right())
                }
            } else if shift {
                transact!(|txn| txn.select_right())
            } else {
                transact!(|txn| txn.move_right())
            }
        }
        Key::Named(NamedKey::ArrowUp) => {
            if shift {
                transact!(|txn| txn.select_up())
            } else {
                transact!(|txn| txn.move_up())
            }
        }
        Key::Named(NamedKey::ArrowDown) => {
            if shift {
                transact!(|txn| txn.select_down())
            } else {
                transact!(|txn| txn.move_down())
            }
        }
        Key::Named(NamedKey::Home) => {
            if action_mod {
                if shift {
                    transact!(|txn| txn.select_to_text_start())
                } else {
                    transact!(|txn| txn.move_to_text_start())
                }
            } else if shift {
                transact!(|txn| txn.select_to_line_start())
            } else {
                transact!(|txn| txn.move_to_line_start())
            }
        }
        Key::Named(NamedKey::End) => {
            if action_mod {
                if shift {
                    transact!(|txn| txn.select_to_text_end())
                } else {
                    transact!(|txn| txn.move_to_text_end())
                }
            } else if shift {
                transact!(|txn| txn.select_to_line_end())
            } else {
                transact!(|txn| txn.move_to_line_end())
            }
        }
        Key::Named(NamedKey::Delete) => {
            if action_mod {
                transact!(|txn| txn.delete_word())
            } else {
                transact!(|txn| txn.delete())
            }
        }
        Key::Named(NamedKey::Backspace) => {
            if action_mod {
                transact!(|txn| txn.backdelete_word())
            } else {
                transact!(|txn| txn.backdelete())
            }
        }
        Key::Named(NamedKey::Enter) => {
            // TODO: support multi-line text inputs
            // transact!(|txn| txn.insert_or_replace_selection("\n"))
        }
        Key::Named(NamedKey::Space) => {
            transact!(|txn| txn.insert_or_replace_selection(" "))
        }
        Key::Character(s) => {
            transact!(|txn| txn.insert_or_replace_selection(&s))
        }
        _ => {}
    };
}
