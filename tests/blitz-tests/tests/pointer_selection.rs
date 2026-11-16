use blitz_test_harness::{Harness, HarnessOptions, pointer_event};
use blitz_traits::events::{BlitzPointerId, MouseEventButton, MouseEventButtons, UiEvent};
use keyboard_types::Modifiers;

fn drag(button: MouseEventButton, buttons: MouseEventButtons) -> (Harness, Vec<String>) {
    let mut harness = Harness::from_html_with(
        r#"<html><body style="margin:0;font:20px monospace"><div id="text">Selectable text for dragging</div></body></html>"#,
        HarnessOptions {
            width: 400,
            height: 200,
            ..Default::default()
        },
    );
    assert!(harness.base().hit(2.0, 10.0).unwrap().is_text);
    let event = |x, buttons| {
        pointer_event(
            BlitzPointerId::Mouse,
            x,
            10.0,
            button,
            buttons,
            Modifiers::default(),
        )
    };
    harness.dispatch(UiEvent::PointerDown(event(2.0, button.into())));
    harness.dispatch(UiEvent::PointerMove(event(90.0, buttons)));
    let events =
        harness.dispatch_recorded([UiEvent::PointerUp(event(90.0, MouseEventButtons::empty()))]);
    (harness, events)
}

#[test]
fn secondary_drag_dispatches_contextmenu_without_selecting() {
    let (harness, events) = drag(MouseEventButton::Secondary, MouseEventButtons::Secondary);
    assert!(events.iter().any(|name| name == "contextmenu"));
    assert!(!events.iter().any(|name| name == "click"));
    assert!(
        harness
            .base()
            .get_selected_text()
            .unwrap_or_default()
            .is_empty()
    );
}

#[test]
fn primary_drag_selects_text_without_clicking() {
    for buttons in [
        MouseEventButtons::Primary,
        MouseEventButtons::Primary | MouseEventButtons::Secondary,
    ] {
        let (harness, events) = drag(MouseEventButton::Main, buttons);
        assert!(
            !harness
                .base()
                .get_selected_text()
                .unwrap_or_default()
                .is_empty()
        );
        assert!(!events.iter().any(|name| name == "click"));
    }
}

#[test]
fn moving_without_primary_does_not_select_text() {
    for buttons in [MouseEventButtons::empty(), MouseEventButtons::Auxiliary] {
        let (harness, events) = drag(MouseEventButton::Main, buttons);
        assert!(
            harness
                .base()
                .get_selected_text()
                .unwrap_or_default()
                .is_empty()
        );
        assert!(events.iter().any(|name| name == "click"));
    }
}

#[test]
fn secondary_drag_in_text_input_does_not_extend_selection() {
    let mut harness = Harness::from_html_with(
        r#"<html><body style="margin:0"><input id="input" value="Selectable text for dragging" style="font:20px monospace;width:300px;height:40px"></body></html>"#,
        HarnessOptions::default(),
    );
    let input = harness.node("#input");
    let event = |x, buttons| {
        pointer_event(
            BlitzPointerId::Mouse,
            x,
            20.0,
            MouseEventButton::Secondary,
            buttons,
            Modifiers::default(),
        )
    };
    harness.dispatch(UiEvent::PointerDown(event(
        10.0,
        MouseEventButtons::Secondary,
    )));
    harness.dispatch(UiEvent::PointerMove(event(
        100.0,
        MouseEventButtons::Secondary,
    )));
    let events =
        harness.dispatch_recorded([UiEvent::PointerUp(event(100.0, MouseEventButtons::empty()))]);
    assert!(events.iter().any(|name| name == "contextmenu"));
    let base = harness.base();
    let editor = &base
        .get_node(input)
        .unwrap()
        .element_data()
        .unwrap()
        .text_input_data()
        .unwrap()
        .editor;
    assert!(editor.raw_selection().is_collapsed());
}
