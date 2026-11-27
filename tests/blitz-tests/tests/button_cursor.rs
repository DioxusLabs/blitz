//! The default cursor over buttons and button-type inputs matches browsers.

use blitz_test_harness::{Harness, HarnessOptions};
use cursor_icon::CursorIcon;

/// Hover the center of `#target` and return the resulting cursor.
fn cursor_over_target(body: &str) -> Option<CursorIcon> {
    let html = format!(r#"<html><body style="margin:0;font-size:20px">{body}</body></html>"#);
    let mut harness = Harness::from_html_with(
        &html,
        HarnessOptions {
            width: 400,
            height: 200,
            ..Default::default()
        },
    );
    let target = harness
        .base()
        .get_element_by_id("target")
        .expect("expected #target");
    let rect = harness
        .base()
        .get_client_bounding_rect(target)
        .expect("expected bounding rect for #target");
    harness.move_mouse_to(
        rect.x as f32 + rect.width as f32 / 2.0,
        rect.y as f32 + rect.height as f32 / 2.0,
    );
    harness.base().get_cursor()
}

#[test]
fn plain_button_has_default_cursor() {
    let cursor = cursor_over_target(r#"<button id="target">Click me</button>"#);
    assert_eq!(cursor, Some(CursorIcon::Default));
}

#[test]
fn nested_span_in_button_has_default_cursor() {
    let cursor = cursor_over_target(r#"<button id="target"><span>Click me</span></button>"#);
    assert_eq!(cursor, Some(CursorIcon::Default));
}

#[test]
fn button_inputs_have_default_cursor() {
    assert_eq!(
        cursor_over_target(r#"<input id="target" type="button" value="Click me">"#),
        Some(CursorIcon::Default)
    );
    assert_eq!(
        cursor_over_target(r#"<input id="target" type="submit" value="Submit">"#),
        Some(CursorIcon::Default)
    );
    assert_eq!(
        cursor_over_target(r#"<input id="target" type="reset" value="Reset">"#),
        Some(CursorIcon::Default)
    );
}

#[test]
fn button_cursor_auto_has_default_cursor() {
    assert_eq!(
        cursor_over_target(r#"<button id="target" style="cursor: auto">Click me</button>"#),
        Some(CursorIcon::Default)
    );
    assert_eq!(
        cursor_over_target(
            r#"<button id="target" style="cursor: auto"><span>Click me</span></button>"#
        ),
        Some(CursorIcon::Default)
    );
}

#[test]
fn button_cursor_auto_with_user_select_text_has_text_cursor() {
    let cursor = cursor_over_target(
        r#"<button id="target" style="cursor: auto; user-select: text">Click me</button>"#,
    );
    assert_eq!(cursor, Some(CursorIcon::Text));
}

#[test]
fn button_explicit_cursor_overrides_default() {
    let cursor =
        cursor_over_target(r#"<button id="target" style="cursor: pointer">Click me</button>"#);
    assert_eq!(cursor, Some(CursorIcon::Pointer));
}
