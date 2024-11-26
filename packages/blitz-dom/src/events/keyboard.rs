use crate::node::TextBrush;
use parley::{FontContext, LayoutContext, PlainEditor, PlainEditorOp};
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
            editor.transact(font_ctx, layout_ctx, [$op]);
        }};
    }

    match event.logical_key {
        #[cfg(not(target_os = "android"))]
        Key::Character(c) if action_mod && matches!(c.as_str(), "c" | "x" | "v") => {
            use arboard::Clipboard;
            use parley::layout::editor::ActiveText;

            match c.to_lowercase().as_str() {
                "c" => {
                    if let ActiveText::Selection(text) = editor.active_text() {
                        let mut cb = Clipboard::new().unwrap();
                        cb.set_text(text.to_owned()).ok();
                    }
                }
                "x" => {
                    if let ActiveText::Selection(text) = editor.active_text() {
                        let mut cb = Clipboard::new().unwrap();
                        cb.set_text(text.to_owned()).ok();
                        transact!(PlainEditorOp::DeleteSelection)
                    }
                }
                "v" => {
                    let mut cb = Clipboard::new().unwrap();
                    let text = cb.get_text().unwrap_or_default();
                    transact!(PlainEditorOp::InsertOrReplaceSelection(text.into(),))
                }
                _ => unreachable!(),
            }
        }
        Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
            if shift {
                transact!(PlainEditorOp::CollapseSelection)
            } else {
                transact!(PlainEditorOp::SelectAll)
            }
        }
        Key::Named(NamedKey::ArrowLeft) => {
            if action_mod {
                if shift {
                    transact!(PlainEditorOp::SelectWordLeft)
                } else {
                    transact!(PlainEditorOp::MoveWordLeft)
                }
            } else if shift {
                transact!(PlainEditorOp::SelectLeft)
            } else {
                transact!(PlainEditorOp::MoveLeft)
            }
        }
        Key::Named(NamedKey::ArrowRight) => {
            if action_mod {
                if shift {
                    transact!(PlainEditorOp::SelectWordRight)
                } else {
                    transact!(PlainEditorOp::MoveWordRight)
                }
            } else if shift {
                transact!(PlainEditorOp::SelectRight)
            } else {
                transact!(PlainEditorOp::MoveRight)
            }
        }
        Key::Named(NamedKey::ArrowUp) => {
            if shift {
                transact!(PlainEditorOp::SelectUp)
            } else {
                transact!(PlainEditorOp::MoveUp)
            }
        }
        Key::Named(NamedKey::ArrowDown) => {
            if shift {
                transact!(PlainEditorOp::SelectDown)
            } else {
                transact!(PlainEditorOp::MoveDown)
            }
        }
        Key::Named(NamedKey::Home) => {
            if action_mod {
                if shift {
                    transact!(PlainEditorOp::SelectToTextStart)
                } else {
                    transact!(PlainEditorOp::MoveToTextStart)
                }
            } else if shift {
                transact!(PlainEditorOp::SelectToLineStart)
            } else {
                transact!(PlainEditorOp::MoveToLineStart)
            }
        }
        Key::Named(NamedKey::End) => {
            if action_mod {
                if shift {
                    transact!(PlainEditorOp::SelectToTextEnd)
                } else {
                    transact!(PlainEditorOp::MoveToTextEnd)
                }
            } else if shift {
                transact!(PlainEditorOp::SelectToLineEnd)
            } else {
                transact!(PlainEditorOp::MoveToLineEnd)
            }
        }
        Key::Named(NamedKey::Delete) => {
            if action_mod {
                transact!(PlainEditorOp::DeleteWord)
            } else {
                transact!(PlainEditorOp::Delete)
            }
        }
        Key::Named(NamedKey::Backspace) => {
            if action_mod {
                transact!(PlainEditorOp::BackdeleteWord)
            } else {
                transact!(PlainEditorOp::Backdelete)
            }
        }
        Key::Named(NamedKey::Enter) => {
            transact!(PlainEditorOp::InsertOrReplaceSelection("\n".into()))
        }
        Key::Named(NamedKey::Space) => {
            transact!(PlainEditorOp::InsertOrReplaceSelection(" ".into()))
        }
        Key::Character(s) => {
            transact!(PlainEditorOp::InsertOrReplaceSelection(s.into()))
        }
        _ => {}
    };
}
