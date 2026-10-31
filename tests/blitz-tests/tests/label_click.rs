//! A label's activation behaviour clicks the control it labels. When the click
//! was already on that control (or inside it), the label does nothing more, as
//! in browsers. Without that check a control with no click action of its own,
//! such as a range input, was clicked by the label, walked back up to the label
//! and was clicked again, without end, until the stack overflowed.

use blitz_test_harness::Harness;

fn is_checked(harness: &Harness, selector: &str) -> bool {
    let id = harness.node(selector);
    harness
        .base()
        .get_node(id)
        .and_then(|node| node.element_data())
        .and_then(|el| el.checkbox_input_checked())
        .unwrap()
}

#[test]
fn clicking_a_range_inside_its_label_terminates() {
    // A failure here is not an assertion but the whole test binary aborting
    // with "has overflowed its stack".
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <label id="label" style="display:block; width:300px; height:40px;">
                <span id="text" style="display:inline-block; width:100px; height:40px;">Volume</span>
                <input id="range" type="range" style="width:150px; height:20px;">
            </label>
        </body></html>"#,
    );

    harness.click("#range");
    harness.click("#text");
    harness.click("#label");
}

#[test]
fn clicking_a_label_still_activates_the_control_inside_it() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <label id="label" style="display:block; width:300px; height:40px;">
                <span id="text" style="display:inline-block; width:100px; height:40px;">Check</span>
                <input id="check" type="checkbox" style="width:20px; height:20px;">
            </label>
        </body></html>"#,
    );

    harness.click("#text");
    assert!(
        is_checked(&harness, "#check"),
        "the label's click reaches its checkbox"
    );

    // A click on the checkbox itself toggles it once: the label does not click it again.
    harness.click("#check");
    assert!(!is_checked(&harness, "#check"));
}

#[test]
fn clicking_a_label_still_activates_the_control_it_is_for() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <label id="label" for="check" style="display:block; width:100px; height:40px;">Check</label>
            <input id="check" type="checkbox" style="width:20px; height:20px;">
        </body></html>"#,
    );

    harness.click("#label");
    assert!(is_checked(&harness, "#check"));
}
