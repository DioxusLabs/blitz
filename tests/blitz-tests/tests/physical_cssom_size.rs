use blitz_test_harness::Harness;

#[test]
fn resolved_width_and_height_use_physical_axes_in_vertical_layout() {
    let harness = Harness::from_html(
        r#"<div style="writing-mode: vertical-rl">
          <div id="content" style="width:50.5px; height:70.25px; padding:2px 6px; border:1px solid"></div>
          <div id="border" style="box-sizing:border-box; width:80.5px; height:100.25px; padding:3px 7px; border:2px solid"></div>
        </div>"#,
    );
    for (selector, width, height) in [
        ("#content", "50.5px", "70.25px"),
        ("#border", "80.5px", "100.25px"),
    ] {
        let node = harness.node(selector);
        assert_eq!(harness.base().resolved_style_value(node, "width"), width);
        assert_eq!(harness.base().resolved_style_value(node, "height"), height);
    }
}
