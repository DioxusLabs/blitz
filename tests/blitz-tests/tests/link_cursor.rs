//! The cursor over a link's descendants is a pointer.

use blitz_test_harness::{Harness, HarnessOptions};
use cursor_icon::CursorIcon;

/// The cursor over the start of the first line of `body`.
fn cursor_at_line_start(body: &str) -> Option<CursorIcon> {
    let html = format!(
        r#"<html><body style="margin:0;font-size:20px"><p style="margin:0">{body}</p></body></html>"#
    );
    let mut harness = Harness::from_html_with(
        &html,
        HarnessOptions {
            width: 400,
            height: 200,
            ..Default::default()
        },
    );
    harness.move_mouse_to(10.0, 10.0);
    harness.base().get_cursor()
}

#[test]
fn text_in_link_has_pointer_cursor() {
    let cursor = cursor_at_line_start(r#"<a href="x"><code>dioxus-native</code></a> framework"#);
    assert_eq!(cursor, Some(CursorIcon::Pointer));
}

#[test]
fn inline_block_in_link_has_pointer_cursor() {
    // docs.rs styles `.docblock p code` as inline-block, so the `<code>` is an
    // atomic inline whose layout parent is the `<p>`, not the `<a>`.
    let cursor = cursor_at_line_start(
        r#"<a href="x"><code style="display:inline-block">dioxus-native</code></a> framework"#,
    );
    assert_eq!(cursor, Some(CursorIcon::Pointer));
}

#[test]
fn text_outside_link_has_text_cursor() {
    let cursor = cursor_at_line_start(r#"It powers <a href="x">dioxus-native</a>"#);
    assert_eq!(cursor, Some(CursorIcon::Text));
}
