//! Drag-selecting in a text input keeps following the pointer after it leaves the field.

use blitz_test_harness::Harness;

/// A value much wider than the 100px input it is placed in.
const LONG: &str = "the quick brown fox jumps over the lazy dog";

fn selected_text(harness: &Harness, selector: &str) -> Option<String> {
    let id = harness.node(selector);
    let doc = harness.base();
    let input = doc.get_node(id).unwrap().element_data().unwrap();
    input
        .text_input_data()
        .unwrap()
        .editor
        .selected_text()
        .map(str::to_string)
}

#[test]
fn drag_selection_continues_past_the_end_of_an_input() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="field" value="{LONG}" style="margin:20px; width:100px; height:30px; padding:0; border:0; font-size:16px">
        </body></html>"#.replace("{LONG}", LONG).as_str(),
    );
    let rect = harness.layout_rect("#field");
    let y = rect.y + rect.height / 2.0;

    // Start at the left edge of the text and drag out of the field to its right, far
    // enough to reach the end of the overflowing value.
    harness.drag((rect.x + 1.0, y), (rect.x + rect.width + 1000.0, y), 8);

    assert_eq!(selected_text(&harness, "#field").as_deref(), Some(LONG));
}

#[test]
fn drag_selection_continues_below_a_textarea() {
    let mut harness = Harness::from_html(
        "<html><body style=\"margin:0\">
            <textarea id=\"field\" style=\"margin:20px; width:200px; height:40px; padding:0; border:0; font-size:16px\">one\ntwo\nthree\nfour\nfive\nsix</textarea>
        </body></html>",
    );
    let rect = harness.layout_rect("#field");

    // Start at the top-left of the text and drag out of the field below it.
    harness.drag(
        (rect.x + 1.0, rect.y + 5.0),
        (rect.x + rect.width / 2.0, rect.y + rect.height + 1000.0),
        8,
    );

    assert_eq!(
        selected_text(&harness, "#field").as_deref(),
        Some("one\ntwo\nthree\nfour\nfive\nsix")
    );
}

#[test]
fn drag_selection_continues_above_the_start_of_an_input() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="field" value="hello world" style="margin:20px; width:200px; height:30px; padding:0; border:0; font-size:16px">
        </body></html>"#,
    );
    let rect = harness.layout_rect("#field");
    let y = rect.y + rect.height / 2.0;

    // Start in the text, then drag up and to the left out of the field.
    harness.drag((rect.x + 190.0, y), (0.0, 0.0), 8);

    assert_eq!(
        selected_text(&harness, "#field").as_deref(),
        Some("hello world")
    );
}

#[test]
fn drag_from_outside_does_not_select_in_an_input() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="field" value="hello world" style="margin:20px; width:200px; height:30px; padding:0; border:0; font-size:16px">
        </body></html>"#,
    );
    let rect = harness.layout_rect("#field");
    let y = rect.y + rect.height / 2.0;

    // Press beside the field and drag across it.
    harness.drag((rect.x - 10.0, y), (rect.x + rect.width - 1.0, y), 8);

    assert_eq!(selected_text(&harness, "#field"), None);
}
