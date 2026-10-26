use blitz_test_harness::Harness;

const RANGE_LABEL: &str = r#"<html><body style="margin:0">
    <label id="hue" style="display:block; width:180px; height:40px;">
        Hue
        <input id="range" type="range" min="0" max="1000" style="width:120px; height:20px;">
    </label>
</body></html>"#;

#[test]
fn clicking_label_wrapping_range_input_terminates() {
    let mut harness = Harness::from_html(RANGE_LABEL);
    harness.click("#hue");
    harness.click("#range");
    harness.pump();
}

#[test]
fn clicking_explicit_range_label_terminates() {
    let mut harness = Harness::from_html(
        r#"<label id="label" for="range">Hue</label><input id="range" type="range">"#,
    );
    harness.click("#label");
    harness.click("#range");
    harness.pump();
}

#[test]
fn label_without_control_terminates() {
    let mut harness = Harness::from_html(r#"<label id="label" for="missing">Hue</label>"#);
    harness.click("#label");
    harness.pump();
}

#[test]
fn label_activates_checkbox_once() {
    let mut harness = Harness::from_html(
        r#"<label id="label" style="display:block;width:180px;height:40px">Enabled
        <input id="checkbox" type="checkbox"></label>"#,
    );
    let checkbox = harness.node("#checkbox");
    let checked = |harness: &Harness| {
        harness
            .base()
            .get_node(checkbox)
            .unwrap()
            .element_data()
            .unwrap()
            .checkbox_input_checked()
    };
    assert_eq!(checked(&harness), Some(false));
    harness.click("#label");
    assert_eq!(checked(&harness), Some(true));
    harness.click("#checkbox");
    assert_eq!(checked(&harness), Some(false));
}
