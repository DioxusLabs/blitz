//! Non-table-internal children of a `display: table` element generate an
//! anonymous table cell (CSS 2.2 §17.2.1), so they must be laid out and
//! hit-testable. Regression test for the gov.uk search box, whose input
//! sits inside plain block `<div>`s that are direct children of a
//! `display: table` wrapper.

use blitz_test_harness::{Harness, HarnessOptions};

fn harness(html: &str) -> Harness {
    Harness::from_html_with(
        html,
        HarnessOptions {
            width: 400,
            height: 200,
            ..Default::default()
        },
    )
}

#[test]
fn block_children_of_table_are_laid_out_as_anonymous_cells() {
    let harness = harness(
        r#"<html><body style="margin:0">
        <div style="display:table; width:400px;">
            <div id="input-wrapper" style="width:100%;">
                <input id="search" type="search" style="width:100%; height:40px; margin:0; box-sizing:border-box;">
            </div>
            <div id="button-wrapper" style="width:40px;">
                <button style="width:40px; height:40px;">Go</button>
            </div>
        </div>
    </body></html>"#,
    );

    let wrapper_rect = harness.layout_rect("#input-wrapper");
    assert!(
        wrapper_rect.width > 0.0 && wrapper_rect.height > 0.0,
        "block child of table should be laid out, got {wrapper_rect:?}"
    );

    let input_rect = harness.layout_rect("#search");
    assert!(
        input_rect.width > 0.0 && input_rect.height > 0.0,
        "input inside table's block child should be laid out, got {input_rect:?}"
    );

    let input = harness.node("#search");
    let (cx, cy) = harness.center_of("#search");
    assert_eq!(
        harness.hit_node(cx, cy),
        input,
        "input inside table's block child should be hit-testable"
    );
}

#[test]
fn clicking_input_inside_table_block_child_focuses_it() {
    let mut harness = harness(
        r#"<html><body style="margin:0">
        <div style="display:table; width:400px;">
            <div style="width:100%;">
                <input id="search" type="search" style="width:100%; height:40px; margin:0; box-sizing:border-box;">
            </div>
        </div>
    </body></html>"#,
    );

    let input = harness.node("#search");
    let (cx, cy) = harness.center_of("#search");
    harness.click_at(cx, cy);
    assert_eq!(harness.focused(), Some(input));
}
