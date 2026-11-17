use blitz_test_harness::Harness;

fn is_checked(harness: &Harness, selector: &str) -> bool {
    let id = harness.node(selector);
    harness
        .base()
        .get_node(id)
        .and_then(|n| n.element_data())
        .and_then(|el| el.checkbox_input_checked())
        .unwrap()
}

#[test]
fn implicit_label_binds_first_labelable_and_guards_interactive_descendants() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <label id="implicit" style="display:block;">
                <span id="caption" style="display:block; height:20px;">Label</span>
                <button id="btn" type="button" style="display:block; width:80px; height:20px;">B</button>
                <input id="cb" type="checkbox" style="display:block; width:20px; height:20px;">
            </label>
            <label for="explicit_cb" style="display:block;">
                <button id="nested_btn" type="button" style="display:block; width:80px; height:20px;">Nested</button>
            </label>
            <input id="explicit_cb" type="checkbox" style="display:block; width:20px; height:20px;">
        </body></html>"#,
    );

    let bound = harness
        .base()
        .label_bound_input_element(harness.node("#implicit"))
        .map(|n| n.id);
    assert_eq!(bound, Some(harness.node("#btn")));

    harness.click("#caption");
    assert_eq!(harness.focused(), Some(harness.node("#btn")));
    assert!(!is_checked(&harness, "#cb"));

    harness.click("#btn");
    assert!(!is_checked(&harness, "#cb"));

    harness.click("#nested_btn");
    assert!(!is_checked(&harness, "#explicit_cb"));
}

#[test]
fn implicit_label_skips_hidden_input() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <label id="lbl" style="display:block; height:20px;">
                Agree
                <input type="HIDDEN" value="token">
                <input id="cb" type="checkbox">
            </label>
        </body></html>"#,
    );

    harness.click("#lbl");
    assert!(is_checked(&harness, "#cb"));
    assert_eq!(harness.focused(), Some(harness.node("#cb")));
}

#[test]
fn explicit_label_resolves_select_textarea_and_button() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <label id="l_sel" for="sel" style="display:block; height:20px;">Select</label>
            <select id="sel"><option>A</option></select>
            <label id="l_txt" for="txt" style="display:block; height:20px;">Notes</label>
            <textarea id="txt"></textarea>
            <label id="l_btn" for="btn" style="display:block; height:20px;">Action</label>
            <button id="btn" type="button">Go</button>
        </body></html>"#,
    );

    for (label, control) in [("#l_sel", "#sel"), ("#l_txt", "#txt"), ("#l_btn", "#btn")] {
        harness.click(label);
        assert_eq!(harness.focused(), Some(harness.node(control)));
    }
}

#[test]
fn label_wrapping_range_input_terminates() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <label id="hue" style="display:block; width:180px; height:40px;">
                <span id="caption" style="display:block; height:20px;">Hue</span>
                <input id="range" type="range" min="0" max="100" style="display:block; width:120px; height:20px;">
            </label>
        </body></html>"#,
    );

    harness.click("#caption");
    assert_eq!(harness.focused(), Some(harness.node("#range")));
    harness.click("#range");
}

#[test]
fn clicking_label_focuses_text_controls_unless_disabled() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <label id="l_input" for="inp" style="display:block; height:20px;">Name</label>
            <input id="inp" type="text">
            <label id="l_area" style="display:block;">
                <span id="area_caption" style="display:block; height:20px;">Bio</span>
                <textarea id="area"></textarea>
            </label>
            <label id="l_dis" for="dis" style="display:block; height:20px;">Disabled</label>
            <input id="dis" type="text" disabled>
        </body></html>"#,
    );

    harness.click("#l_input");
    assert_eq!(harness.focused(), Some(harness.node("#inp")));

    harness.click("#area_caption");
    assert_eq!(harness.focused(), Some(harness.node("#area")));

    harness.click("#l_dis");
    assert_ne!(harness.focused(), Some(harness.node("#dis")));
}
