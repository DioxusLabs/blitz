//! Tests for `<input type="range">` slider functionality, which is implemented
//! via a `RangeInputWidget` custom widget that is automatically attached by the
//! `DocumentMutator` when a range input is added to the DOM.

use blitz_dom::{Document, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{
        BlitzKeyEvent, BlitzPointerEvent, BlitzPointerId, KeyState, MouseEventButton,
        MouseEventButtons, Point, PointerCoords, PointerDetails, UiEvent,
    },
    shell::{ColorScheme, Viewport},
};
use keyboard_types::{Code, Key, Location};
use std::sync::Arc;

fn doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(200, 200, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn slider_doc(attrs: &str) -> HtmlDocument {
    // A 100px wide, 16px tall slider positioned at (0, 0).
    // The thumb radius is 8px, so the thumb center travels from x=8 to x=92.
    doc(&format!(
        r#"<html><body style="margin:0">
            <input id="slider" type="range" style="display:block; width:100px; height:16px; margin:0" {attrs} />
        </body></html>"#
    ))
}

fn pointer_event(x: f32, y: f32, buttons: MouseEventButtons) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button: MouseEventButton::Main,
        buttons,
        mods: Default::default(),
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    }
}

fn pointer_down(doc: &mut HtmlDocument, x: f32, y: f32) {
    let event = pointer_event(x, y, MouseEventButtons::Primary);
    doc.handle_ui_event(UiEvent::PointerDown(event));
}

fn pointer_move(doc: &mut HtmlDocument, x: f32, y: f32, buttons: MouseEventButtons) {
    let event = pointer_event(x, y, buttons);
    doc.handle_ui_event(UiEvent::PointerMove(event));
}

fn pointer_up(doc: &mut HtmlDocument, x: f32, y: f32) {
    let event = pointer_event(x, y, MouseEventButtons::None);
    doc.handle_ui_event(UiEvent::PointerUp(event));
}

fn pointer_cancel(doc: &mut HtmlDocument, x: f32, y: f32) {
    let event = pointer_event(x, y, MouseEventButtons::None);
    doc.handle_ui_event(UiEvent::PointerCancel(event));
}

fn capture_target(doc: &HtmlDocument) -> Option<usize> {
    doc.inner().pointer_capture_target(BlitzPointerId::Mouse)
}

fn key_down(doc: &mut HtmlDocument, key: Key, code: Code) {
    doc.handle_ui_event(UiEvent::KeyDown(BlitzKeyEvent {
        key,
        code,
        modifiers: Default::default(),
        location: Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state: KeyState::Pressed,
        text: None,
    }));
}

fn slider_value(doc: &HtmlDocument) -> Option<String> {
    let id = doc.query_selector("#slider").unwrap().expect("#slider");
    doc.get_node(id)
        .and_then(|node| node.attr(blitz_dom::local_name!("value")))
        .map(str::to_string)
}

#[test]
fn range_input_has_custom_widget_attached() {
    let doc = slider_doc("");
    let id = doc.query_selector("#slider").unwrap().expect("#slider");
    let element = doc.get_node(id).unwrap().element_data().unwrap();
    assert!(
        element.custom_widget_data().is_some(),
        "range input should have a custom widget attached"
    );
}

#[test]
fn custom_widget_does_not_force_continuous_animation() {
    // Custom widgets should not cause the document to be considered "animating"
    // (which would cause it to be continuously redrawn). Widgets which animate
    // should request redraws via `WidgetPaintContext::request_redraw` instead.
    let doc = slider_doc("");
    assert!(!doc.inner().is_animating());
}

#[test]
fn click_sets_value() {
    let mut doc = slider_doc("");

    // Click at the thumb-track position corresponding to 75%
    // (thumb travel is from x=8 to x=92, so 75% is at x=71)
    pointer_down(&mut doc, 71.0, 8.0);
    pointer_up(&mut doc, 71.0, 8.0);
    assert_eq!(slider_value(&doc).as_deref(), Some("75"));

    // Clicking at/beyond the ends clamps to min/max
    pointer_down(&mut doc, 0.0, 8.0);
    pointer_up(&mut doc, 0.0, 8.0);
    assert_eq!(slider_value(&doc).as_deref(), Some("0"));

    pointer_down(&mut doc, 100.0, 8.0);
    pointer_up(&mut doc, 100.0, 8.0);
    assert_eq!(slider_value(&doc).as_deref(), Some("100"));
}

#[test]
fn drag_updates_value() {
    let mut doc = slider_doc("");

    // Press at the midpoint, then drag to 25%
    pointer_down(&mut doc, 50.0, 8.0);
    assert_eq!(slider_value(&doc).as_deref(), Some("50"));

    pointer_move(&mut doc, 29.0, 8.0, MouseEventButtons::Primary);
    assert_eq!(slider_value(&doc).as_deref(), Some("25"));

    pointer_up(&mut doc, 29.0, 8.0);

    // Moving without the button pressed does not change the value
    pointer_move(&mut doc, 92.0, 8.0, MouseEventButtons::None);
    assert_eq!(slider_value(&doc).as_deref(), Some("25"));
}

#[test]
fn click_respects_min_max_and_step() {
    let mut doc = slider_doc(r#"min="0" max="10" step="2""#);

    // 75% of the 0-10 range is 7.5, which snaps to 8
    pointer_down(&mut doc, 71.0, 8.0);
    pointer_up(&mut doc, 71.0, 8.0);
    assert_eq!(slider_value(&doc).as_deref(), Some("8"));
}

#[test]
fn arrow_keys_update_value() {
    let mut doc = slider_doc(r#"value="50""#);

    // Focus the slider by clicking on it (at its current value so it doesn't change)
    pointer_down(&mut doc, 50.0, 8.0);
    pointer_up(&mut doc, 50.0, 8.0);
    assert_eq!(slider_value(&doc).as_deref(), Some("50"));

    key_down(&mut doc, Key::ArrowRight, Code::ArrowRight);
    assert_eq!(slider_value(&doc).as_deref(), Some("51"));

    key_down(&mut doc, Key::ArrowLeft, Code::ArrowLeft);
    key_down(&mut doc, Key::ArrowLeft, Code::ArrowLeft);
    assert_eq!(slider_value(&doc).as_deref(), Some("49"));

    key_down(&mut doc, Key::Home, Code::Home);
    assert_eq!(slider_value(&doc).as_deref(), Some("0"));

    key_down(&mut doc, Key::End, Code::End);
    assert_eq!(slider_value(&doc).as_deref(), Some("100"));
}

#[test]
fn drag_captures_pointer_and_works_outside_bounds() {
    let mut doc = slider_doc("");
    let slider_id = doc.query_selector("#slider").unwrap().expect("#slider");

    // Pressing on the slider captures the pointer
    pointer_down(&mut doc, 50.0, 8.0);
    assert_eq!(capture_target(&doc), Some(slider_id));
    assert_eq!(slider_value(&doc).as_deref(), Some("50"));

    // Moves outside of the slider's bounds are retargeted at the slider while captured
    pointer_move(&mut doc, 150.0, 150.0, MouseEventButtons::Primary);
    assert_eq!(slider_value(&doc).as_deref(), Some("100"));

    pointer_move(&mut doc, 29.0, 190.0, MouseEventButtons::Primary);
    assert_eq!(slider_value(&doc).as_deref(), Some("25"));

    // Releasing the pointer (outside of the slider's bounds) releases the capture
    pointer_up(&mut doc, 29.0, 190.0);
    assert_eq!(capture_target(&doc), None);

    // Subsequent moves outside of the slider no longer update the value
    pointer_move(&mut doc, 150.0, 150.0, MouseEventButtons::Primary);
    assert_eq!(slider_value(&doc).as_deref(), Some("25"));
}

#[test]
fn pointer_cancel_releases_capture() {
    let mut doc = slider_doc("");
    let slider_id = doc.query_selector("#slider").unwrap().expect("#slider");

    pointer_down(&mut doc, 50.0, 8.0);
    assert_eq!(capture_target(&doc), Some(slider_id));

    pointer_cancel(&mut doc, 150.0, 150.0);
    assert_eq!(capture_target(&doc), None);

    // The drag has ended: moves over the slider with the button pressed don't update the value
    pointer_move(&mut doc, 71.0, 8.0, MouseEventButtons::Primary);
    assert_eq!(slider_value(&doc).as_deref(), Some("50"));
}

#[test]
fn removing_capturing_node_releases_capture() {
    let mut doc = slider_doc("");
    let slider_id = doc.query_selector("#slider").unwrap().expect("#slider");

    pointer_down(&mut doc, 50.0, 8.0);
    assert_eq!(capture_target(&doc), Some(slider_id));

    doc.inner_mut().mutate().remove_node(slider_id);
    assert_eq!(capture_target(&doc), None);
}

#[test]
fn disabled_slider_ignores_input() {
    let mut doc = slider_doc(r#"value="50" disabled"#);

    pointer_down(&mut doc, 71.0, 8.0);
    pointer_up(&mut doc, 71.0, 8.0);
    assert_eq!(slider_value(&doc).as_deref(), Some("50"));
}
