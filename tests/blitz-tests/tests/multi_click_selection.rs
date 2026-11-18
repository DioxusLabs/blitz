//! A double click selects the word under the pointer in page text, and a triple
//! click the line between hard breaks it is in, wrapped or not, as browsers do.
//! `user-select: none` on an ancestor blocks both.

use blitz_test_harness::Harness;

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
