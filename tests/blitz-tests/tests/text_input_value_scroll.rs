//! A text input keeps its caret in view when its value is set from outside.
//!
//! Typing past the end of a narrow input scrolls it. When the app then clears
//! or shortens the value (a clear button, a submitted form), the caret moves
//! back but the scroll offset stayed where it was, so the input looked empty
//! and its caret was out of sight.

use blitz_dom::{NodeId, QualName, local_name, ns};
use blitz_test_harness::{Harness, HarnessOptions};

const INPUT: &str = r#"<html><body style="margin:0">
    <input id="input" style="width:60px;">
</body></html>"#;

const LONG_TEXT: &str = "a value much longer than the input is wide";

fn value() -> QualName {
    QualName::new(None, ns!(), local_name!("value"))
}

fn scroll_offset(harness: &Harness, input: NodeId) -> f32 {
    harness
        .base()
        .get_node(input)
        .and_then(|node| node.element_data())
        .and_then(|el| el.text_input_data())
        .map(|data| data.scroll_offset)
        .unwrap()
}

/// An input scrolled to the end of `LONG_TEXT`, typed into it.
fn scrolled_input() -> (Harness, NodeId) {
    let mut harness = Harness::from_html_with(
        INPUT,
        HarnessOptions {
            width: 300,
            height: 100,
            ..Default::default()
        },
    );
    let input = harness.node("#input");
    harness.click("#input");
    harness.type_text(LONG_TEXT);
    assert!(
        scroll_offset(&harness, input) > 0.0,
        "typing past the end scrolls the input"
    );
    (harness, input)
}

#[test]
fn clearing_the_value_scrolls_back_to_the_start() {
    let (mut harness, input) = scrolled_input();

    harness
        .base_mut()
        .mutate()
        .set_attribute(input, value(), "");

    assert_eq!(scroll_offset(&harness, input), 0.0);
}

#[test]
fn shortening_the_value_keeps_the_caret_in_view() {
    let (mut harness, input) = scrolled_input();

    harness
        .base_mut()
        .mutate()
        .set_attribute(input, value(), "a");

    assert_eq!(scroll_offset(&harness, input), 0.0);
}

#[test]
fn removing_the_value_scrolls_back_to_the_start() {
    let (mut harness, input) = scrolled_input();
    harness
        .base_mut()
        .mutate()
        .set_attribute(input, value(), LONG_TEXT);

    harness.base_mut().mutate().clear_attribute(input, value());

    assert_eq!(scroll_offset(&harness, input), 0.0);
}
