use blitz_dom::{QualName, local_name, ns};
use blitz_test_harness::{Harness, HarnessOptions};
use cursor_icon::CursorIcon;

#[test]
fn button_anonymous_block_inherits_not_allowed_cursor() {
    let mut harness = Harness::from_html_with(
        r#"<html><body style="margin:0;font-size:20px"><button id="btn" style="display:inline-flex;padding:8px;cursor:not-allowed">Text</button></body></html>"#,
        HarnessOptions {
            width: 400,
            height: 200,
            ..Default::default()
        },
    );

    harness.move_mouse_to(4.0, 4.0);
    assert_eq!(harness.base().get_cursor(), Some(CursorIcon::NotAllowed));

    harness.move_mouse_to(14.0, 14.0);
    assert_eq!(harness.base().get_cursor(), Some(CursorIcon::NotAllowed));
}

#[test]
fn dynamic_style_mutation_updates_inherited_cursor() {
    let mut harness = Harness::from_html_with(
        r#"<html><body style="margin:0;font-size:20px"><button id="btn" style="display:inline-flex;padding:8px">Text</button></body></html>"#,
        HarnessOptions {
            width: 400,
            height: 200,
            ..Default::default()
        },
    );

    harness.move_mouse_to(14.0, 14.0);
    assert_eq!(harness.base().get_cursor(), Some(CursorIcon::Text));

    let btn = harness.node("#btn");
    harness.base_mut().mutate().set_attribute(
        btn,
        QualName::new(None, ns!(), local_name!("style")),
        "display:inline-flex;padding:8px;cursor:not-allowed",
    );
    harness.pump();
    assert_eq!(harness.base().get_cursor(), Some(CursorIcon::NotAllowed));
}
