//! A double click selects the word under the pointer in page text, and a triple
//! click the line between hard breaks it is in, wrapped or not, as browsers do.
//! `user-select: none` on an ancestor blocks both.

use blitz_test_harness::Harness;
use blitz_test_harness::{mouse_pointer_event, pointer_event};
use blitz_traits::events::{BlitzPointerId, MouseEventButton, MouseEventButtons, UiEvent};

/// "hello world" wraps after "hello": the box is too narrow for both words.
const STYLE: &str = "margin:0; width:80px; font-size:20px; line-height:20px";

fn harness(body_style: &str) -> Harness {
    Harness::from_html(&format!(
        "<html><body style='margin:0; {body_style}'>\
         <p id='text' style='{STYLE}'>hello world<br>next</p></body></html>"
    ))
}

/// Clicks `count` times on the first word, quickly enough to count as one gesture.
fn click_first_word(harness: &mut Harness, count: usize) {
    for _ in 0..count {
        harness.click_at(10.0, 10.0);
    }
}

fn selected_text(harness: &Harness) -> Option<String> {
    harness.base().get_selected_text()
}

#[test]
fn double_click_selects_a_word() {
    let mut harness = harness("");
    click_first_word(&mut harness, 2);
    assert_eq!(selected_text(&harness).as_deref(), Some("hello"));
}

#[test]
fn triple_click_selects_up_to_a_hard_break_across_a_wrap() {
    let mut harness = harness("");
    click_first_word(&mut harness, 3);
    let text = selected_text(&harness).unwrap();
    assert_eq!(text.trim_end(), "hello world");
}

#[test]
fn single_click_selects_nothing() {
    let mut harness = harness("");
    click_first_word(&mut harness, 1);
    assert_eq!(selected_text(&harness).as_deref().unwrap_or(""), "");
}

#[test]
fn user_select_none_on_an_ancestor_blocks_a_double_click() {
    let mut harness = harness("user-select:none");
    click_first_word(&mut harness, 2);
    assert_eq!(selected_text(&harness).as_deref().unwrap_or(""), "");
}

#[test]
fn a_click_after_a_right_click_starts_a_new_count() {
    let mut harness = harness("");
    let right = pointer_event(
        BlitzPointerId::Mouse,
        10.0,
        10.0,
        MouseEventButton::Secondary,
        MouseEventButtons::from(MouseEventButton::Secondary),
        Default::default(),
    );
    harness.dispatch(UiEvent::PointerDown(right.clone()));
    harness.dispatch(UiEvent::PointerUp(right));
    harness.click_at(10.0, 10.0);
    assert_eq!(selected_text(&harness).as_deref().unwrap_or(""), "");
}

#[test]
fn a_small_move_during_a_double_click_keeps_the_word() {
    let mut harness = harness("");
    harness.click_at(10.0, 10.0);
    harness.dispatch(UiEvent::PointerDown(mouse_pointer_event(10.0, 10.0)));
    harness.dispatch(UiEvent::PointerMove(mouse_pointer_event(14.0, 10.0)));
    harness.dispatch(UiEvent::PointerUp(mouse_pointer_event(14.0, 10.0)));
    assert_eq!(selected_text(&harness).as_deref(), Some("hello"));
}
