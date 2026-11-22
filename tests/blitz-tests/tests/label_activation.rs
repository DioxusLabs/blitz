//! Clicking a `<label>` runs the activation behavior of its labeled control.
//! That synthetic click must not activate the label again, even when the
//! control has no activation behavior of its own (e.g. a range input).

use blitz_test_harness::{Harness, HarnessOptions};

fn harness(html: &str) -> Harness {
    Harness::from_html_with(
        html,
        HarnessOptions {
            width: 200,
            height: 200,
            ..Default::default()
        },
    )
}

const LABEL_WRAPPING_RANGE: &str = r#"<html><body style="margin:0">
    <label id="hue" style="display:block; width:180px; height:40px;">
        Hue
        <input id="range" type="range" min="0" max="1000" style="width:120px; height:20px;">
    </label>
</body></html>"#;

#[test]
fn clicking_label_wrapping_range_input_terminates() {
    let mut harness = harness(LABEL_WRAPPING_RANGE);
    harness.click("#hue");
}

#[test]
fn clicking_range_input_inside_label_terminates() {
    let mut harness = harness(LABEL_WRAPPING_RANGE);
    harness.click("#range");
}

#[test]
fn clicking_label_still_toggles_wrapped_checkbox() {
    let mut harness = harness(
        r#"<html><body style="margin:0">
            <label id="lbl" style="display:block; width:180px; height:40px;">
                Agree <input id="cb" type="checkbox">
            </label>
        </body></html>"#,
    );
    harness.click("#lbl");
    let id = harness.node("#cb");
    let checked = harness
        .base()
        .get_node(id)
        .and_then(|node| node.element_data())
        .and_then(|el| el.checkbox_input_checked())
        .unwrap();
    assert!(checked);
}
