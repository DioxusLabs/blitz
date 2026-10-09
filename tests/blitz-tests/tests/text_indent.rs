use blitz_test_harness::{Harness, HarnessOptions};

fn layout(css: &str, indent: &str, scale: f32) -> Harness {
    Harness::from_html_with(
        &format!(
            r#"<style>
                body {{ margin: 0; }}
                #test {{ {css}; text-indent: {indent}; }}
                #marker {{ display: inline-block; width: 10px; height: 10px; }}
            </style><div id="test"><span id="marker"></span></div>"#,
        ),
        HarnessOptions {
            scale,
            ..Default::default()
        },
    )
}

fn first_line_indent(harness: &Harness) -> f32 {
    let doc = harness.base();
    doc.get_node(harness.node("#test"))
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .unwrap()
        .layout
        .get(0)
        .unwrap()
        .metrics()
        .indent
}

#[test]
fn percentages_resolve_against_the_content_box() {
    for css in [
        "box-sizing: border-box; width: 120px; padding-right: 10px; border-right: 10px solid",
        "box-sizing: border-box; width: 120px; padding-right: 10px; border-right: 10px solid; overflow: hidden",
        "width: 100px; padding: 7px; border: 3px solid",
    ] {
        for (indent, expected) in [("50%", 50.0), ("calc(25px + 25%)", 50.0), ("-50%", -50.0)] {
            for scale in [1.0, 2.0] {
                let harness = layout(css, indent, scale);
                assert_eq!(
                    first_line_indent(&harness),
                    expected * scale,
                    "{css}; {indent}; scale {scale}"
                );
            }
        }
    }
}

#[test]
fn percentage_indents_do_not_increase_intrinsic_widths() {
    for width in ["min-content", "max-content", "fit-content"] {
        for (indent, expected_width, expected_indent) in
            [("50%", 10.0, 5.0), ("calc(10px + 50%)", 20.0, 20.0)]
        {
            let harness = layout(&format!("width: {width}"), indent, 1.0);
            assert_eq!(
                harness.layout_rect("#test").width,
                expected_width,
                "{width}; {indent}"
            );
            assert_eq!(
                first_line_indent(&harness),
                expected_indent,
                "{width}; {indent}"
            );
        }
    }
}

#[test]
fn shrink_to_fit_resolves_percentages_after_intrinsic_measurement() {
    let harness = layout("display: inline-block", "50%", 1.0);
    assert_eq!(harness.layout_rect("#test").width, 10.0);
    assert_eq!(first_line_indent(&harness), 5.0);
}
