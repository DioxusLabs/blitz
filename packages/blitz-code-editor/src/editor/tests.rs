use kurbo::{Affine, Point, Size};

use crate::{
    CodeEditor, EditorInput, ImeInput, Key, KeyInput, Modifiers, PointerButton, TextContext,
};

fn editor(text: &str) -> (CodeEditor, TextContext) {
    let mut editor = CodeEditor::new(text);
    editor.set_size(Size::new(800.0, 600.0));
    (editor, TextContext::new())
}

fn key(editor: &mut CodeEditor, cx: &mut TextContext, key: Key, mods: Modifiers) {
    let text = match &key {
        Key::Character(c) => Some(c.clone()),
        _ => None,
    };
    editor.handle_input(cx, EditorInput::Key(KeyInput { key, mods, text }));
}

fn type_text(editor: &mut CodeEditor, cx: &mut TextContext, text: &str) {
    for c in text.chars() {
        key(
            editor,
            cx,
            Key::Character(c.to_string()),
            Modifiers::default(),
        );
    }
}

fn primary() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers {
            meta: true,
            ..Default::default()
        }
    } else {
        Modifiers {
            ctrl: true,
            ..Default::default()
        }
    }
}

const SHIFT: Modifiers = Modifiers {
    shift: true,
    ctrl: false,
    alt: false,
    meta: false,
};

#[test]
fn typing_inserts_at_the_caret() {
    let (mut editor, mut cx) = editor("");
    type_text(&mut editor, &mut cx, "hello");
    assert_eq!(editor.text(), "hello");
    assert_eq!(editor.caret(), 5);

    key(&mut editor, &mut cx, Key::Backspace, Modifiers::default());
    assert_eq!(editor.text(), "hell");
}

#[test]
fn shortcuts_do_not_insert_text() {
    let (mut editor, mut cx) = editor("abc");
    key(&mut editor, &mut cx, Key::Character("q".into()), primary());
    assert_eq!(editor.text(), "abc");
}

#[test]
fn undo_and_redo() {
    let (mut editor, mut cx) = editor("");
    type_text(&mut editor, &mut cx, "abc");
    key(&mut editor, &mut cx, Key::Character("z".into()), primary());
    assert_eq!(editor.text(), "");
    let redo = Modifiers {
        shift: true,
        ..primary()
    };
    key(&mut editor, &mut cx, Key::Character("z".into()), redo);
    assert_eq!(editor.text(), "abc");
}

#[test]
fn moving_the_caret_starts_a_new_undo_group() {
    let (mut editor, mut cx) = editor("");
    type_text(&mut editor, &mut cx, "ab");
    key(&mut editor, &mut cx, Key::ArrowLeft, Modifiers::default());
    type_text(&mut editor, &mut cx, "c");
    assert_eq!(editor.text(), "acb");
    key(&mut editor, &mut cx, Key::Character("z".into()), primary());
    assert_eq!(editor.text(), "ab");
}

#[test]
fn caret_moves_do_not_grow_the_selection_history() {
    let (mut editor, mut cx) = editor("hello\nworld");
    for _ in 0..10 {
        key(&mut editor, &mut cx, Key::ArrowRight, Modifiers::default());
    }
    key(&mut editor, &mut cx, Key::ArrowUp, SHIFT);
    assert!(editor.cursor.history_selections.is_empty());
}

#[test]
fn shift_arrows_extend_the_selection() {
    let (mut editor, mut cx) = editor("hello world");
    key(&mut editor, &mut cx, Key::ArrowRight, SHIFT);
    key(&mut editor, &mut cx, Key::ArrowRight, SHIFT);
    assert_eq!(editor.selections(), vec![(0, 2)]);

    // Without shift, Right collapses the selection to its end
    key(&mut editor, &mut cx, Key::ArrowRight, Modifiers::default());
    assert_eq!(editor.selections(), vec![(2, 2)]);
}

#[test]
fn select_all_then_type_replaces_everything() {
    let (mut editor, mut cx) = editor("one\ntwo\nthree");
    key(&mut editor, &mut cx, Key::Character("a".into()), primary());
    type_text(&mut editor, &mut cx, "x");
    assert_eq!(editor.text(), "x");
}

#[test]
fn up_and_down_keep_the_column() {
    let (mut editor, mut cx) = editor("abcdef\nab\nabcdef");
    for _ in 0..4 {
        key(&mut editor, &mut cx, Key::ArrowRight, Modifiers::default());
    }
    // The short middle line clamps the caret to its end...
    key(&mut editor, &mut cx, Key::ArrowDown, Modifiers::default());
    assert_eq!(editor.caret(), 9);
    // ...but the original column comes back on the next long line
    key(&mut editor, &mut cx, Key::ArrowDown, Modifiers::default());
    assert_eq!(editor.caret(), 14);
    key(&mut editor, &mut cx, Key::ArrowUp, Modifiers::default());
    key(&mut editor, &mut cx, Key::ArrowUp, Modifiers::default());
    assert_eq!(editor.caret(), 4);
}

#[test]
fn up_on_the_first_line_goes_to_the_start() {
    let (mut editor, mut cx) = editor("abc\ndef");
    key(&mut editor, &mut cx, Key::ArrowRight, Modifiers::default());
    key(&mut editor, &mut cx, Key::ArrowUp, Modifiers::default());
    assert_eq!(editor.caret(), 0);
}

#[test]
fn enter_keeps_indentation() {
    let (mut editor, mut cx) = editor("    foo");
    key(&mut editor, &mut cx, Key::End, Modifiers::default());
    key(&mut editor, &mut cx, Key::Enter, Modifiers::default());
    type_text(&mut editor, &mut cx, "bar");
    assert_eq!(editor.text(), "    foo\n    bar");
}

#[test]
fn copy_and_paste_go_through_the_host() {
    let (mut editor, mut cx) = editor("hello world");
    key(&mut editor, &mut cx, Key::End, SHIFT);
    let resp = editor.handle_input(
        &mut cx,
        EditorInput::Key(KeyInput {
            key: Key::Character("c".into()),
            mods: primary(),
            text: None,
        }),
    );
    assert_eq!(resp.set_clipboard.as_deref(), Some("hello world"));

    key(&mut editor, &mut cx, Key::End, Modifiers::default());
    let resp = editor.handle_input(
        &mut cx,
        EditorInput::Key(KeyInput {
            key: Key::Character("v".into()),
            mods: primary(),
            text: None,
        }),
    );
    assert!(resp.request_clipboard);
    let resp = editor.handle_input(&mut cx, EditorInput::Paste("!".into()));
    assert!(resp.text_changed);
    assert_eq!(editor.text(), "hello world!");
}

#[test]
fn ime_commit_inserts_and_preedit_does_not() {
    let (mut editor, mut cx) = editor("");
    editor.handle_input(&mut cx, EditorInput::Focus(true));
    editor.handle_input(
        &mut cx,
        EditorInput::Ime(ImeInput::Preedit {
            text: "ni".into(),
            cursor: Some((2, 2)),
        }),
    );
    assert_eq!(editor.text(), "");
    // Keys typed while composing belong to the IME
    type_text(&mut editor, &mut cx, "x");
    assert_eq!(editor.text(), "");

    let resp = editor.handle_input(&mut cx, EditorInput::Ime(ImeInput::Commit("你".into())));
    assert_eq!(editor.text(), "你");
    assert!(resp.ime_cursor_area.is_some());
}

#[test]
fn focus_toggles_the_ime() {
    let (mut editor, mut cx) = editor("");
    let resp = editor.handle_input(&mut cx, EditorInput::Focus(true));
    assert_eq!(resp.ime_enabled, Some(true));
    let resp = editor.handle_input(&mut cx, EditorInput::Focus(false));
    assert_eq!(resp.ime_enabled, Some(false));
}

#[test]
fn clicks_place_the_caret() {
    let (mut editor, mut cx) = editor("first line\nsecond line\nthird line");
    editor.prepare(&mut cx);
    let lh = editor.metrics.line_height;
    let cw = editor.metrics.char_width;
    let x = editor.text_x() + 3.0 * cw + cw * 0.25;

    let click = |editor: &mut CodeEditor, cx: &mut TextContext, pos: Point, click_count| {
        editor.handle_input(
            cx,
            EditorInput::PointerDown {
                pos,
                button: PointerButton::Primary,
                mods: Modifiers::default(),
                click_count,
            },
        );
        editor.handle_input(
            cx,
            EditorInput::PointerUp {
                pos,
                button: PointerButton::Primary,
            },
        );
    };

    click(&mut editor, &mut cx, Point::new(x, lh * 1.5), 1);
    assert_eq!(editor.caret(), 11 + 3);

    // Double click selects the word
    click(&mut editor, &mut cx, Point::new(x, lh * 1.5), 2);
    assert_eq!(editor.selections(), vec![(11, 17)]);

    // Below the last line hits the last line
    click(&mut editor, &mut cx, Point::new(x, lh * 10.0), 1);
    assert_eq!(editor.caret(), 23 + 3);
}

#[test]
fn drag_extends_the_selection() {
    let (mut editor, mut cx) = editor("first line\nsecond line");
    editor.prepare(&mut cx);
    let lh = editor.metrics.line_height;
    let x0 = editor.text_x();
    editor.handle_input(
        &mut cx,
        EditorInput::PointerDown {
            pos: Point::new(x0, lh * 0.5),
            button: PointerButton::Primary,
            mods: Modifiers::default(),
            click_count: 1,
        },
    );
    editor.handle_input(
        &mut cx,
        EditorInput::PointerMove {
            pos: Point::new(x0, lh * 1.5),
        },
    );
    assert_eq!(editor.selections(), vec![(0, 11)]);
}

#[test]
fn edits_keep_the_line_cache_in_sync() {
    let (mut editor, mut cx) = editor("a\nb\nc");
    let mut scene = anyrender::Scene::new();
    editor.paint(&mut cx, &mut scene, Affine::IDENTITY);
    assert_eq!(editor.cache.len(), 3);

    key(&mut editor, &mut cx, Key::Enter, Modifiers::default());
    key(&mut editor, &mut cx, Key::Enter, Modifiers::default());
    assert_eq!(editor.cache.len(), 5);

    key(&mut editor, &mut cx, Key::Character("a".into()), primary());
    key(&mut editor, &mut cx, Key::Backspace, Modifiers::default());
    assert_eq!(editor.cache.len(), 1);
    editor.paint(&mut cx, &mut scene, Affine::IDENTITY);
}

#[test]
fn paints_empty_and_tabbed_text() {
    let mut cx = TextContext::new();
    let mut scene = anyrender::Scene::new();
    for text in ["", "\tfoo\n\t\tbar\n", "a\r\nb"] {
        let mut editor = CodeEditor::new(text);
        editor.set_size(Size::new(400.0, 300.0));
        editor.handle_input(&mut cx, EditorInput::Focus(true));
        editor.paint(&mut cx, &mut scene, Affine::scale(2.0));
    }
}

#[test]
fn tabs_advance_to_the_next_tab_stop() {
    let (mut editor, mut cx) = editor("\tx\nab\tx");
    editor.prepare(&mut cx);
    let cw = editor.metrics.char_width;
    let tab_x = super::caret_x(editor.cache.line(&mut cx, &editor.buffer, 0), 1);
    let tab_x2 = super::caret_x(editor.cache.line(&mut cx, &editor.buffer, 1), 3);
    assert!((tab_x - 4.0 * cw).abs() < 0.5, "{tab_x} vs {}", 4.0 * cw);
    assert!((tab_x2 - 4.0 * cw).abs() < 0.5, "{tab_x2} vs {}", 4.0 * cw);
}
