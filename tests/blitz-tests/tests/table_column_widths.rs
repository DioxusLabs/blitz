//! Column widths of auto-layout tables, as Chromium gives them (CSS Tables 3
//! width distribution).
//!
//! Cells hold inline-block boxes of fixed sizes, with `font-size: 0` so that
//! the spaces between them take no width, so the results do not depend on
//! fonts.

use blitz_test_harness::Harness;

/// The border-box widths of the cells with the given ids.
fn widths(table: &str, cells: &[&str]) -> Vec<f32> {
    let harness = Harness::from_html(&format!(
        "<html><head><style>body {{ margin: 0 }} table {{ border-spacing: 0 }} \
         td {{ padding: 0; font-size: 0 }}</style></head><body>{table}</body></html>"
    ));
    cells
        .iter()
        .map(|id| harness.layout_rect(&format!("#{id}")).width)
        .collect()
}

/// `count` inline-block boxes `width` px wide.
fn boxes(count: usize, width: u32) -> String {
    format!("<div style='display:inline-block;width:{width}px;height:10px'></div> ").repeat(count)
}

#[track_caller]
fn assert_close(actual: &[f32], expected: &[f32]) {
    let close = actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(a, e)| (a - e).abs() < 0.5);
    assert!(close, "widths {actual:?}, expected {expected:?}");
}

#[test]
fn columns_grow_from_min_to_max_content_by_the_same_fraction() {
    // Min/max-content widths 50/100 and 40/400 in a 300px table: the 210px
    // above the min-content widths go 50 : 360, as in Chromium (75.6 and
    // 224.4). Sharing space equally, as grid tracks do, gave 100 and 200.
    let widths = widths(
        &format!(
            "<table width=300><tr><td id=a>{}</td><td id=b>{}</td></tr></table>",
            boxes(2, 50),
            boxes(10, 40)
        ),
        &["a", "b"],
    );
    assert_close(&widths, &[75.6, 224.4]);
}

#[test]
fn spare_width_goes_to_columns_in_proportion_to_their_content() {
    // A 400px table whose content is narrower: each column grows in
    // proportion to its max-content width (here 1 : 3).
    let widths = widths(
        &format!(
            "<table width=400><tr><td id=a>{}</td><td id=b>{}</td></tr></table>",
            boxes(1, 30),
            boxes(1, 90)
        ),
        &["a", "b"],
    );
    assert_close(&widths, &[100.0, 300.0]);
}

#[test]
fn percentage_widths_on_cells_are_honoured() {
    let widths = widths(
        &format!(
            "<table width=400><tr><td id=a width='25%'>{}</td><td id=b>{}</td>\
             <td id=c width='10%'>{}</td></tr></table>",
            boxes(1, 10),
            boxes(20, 40),
            boxes(1, 10)
        ),
        &["a", "b", "c"],
    );
    assert_close(&widths, &[100.0, 260.0, 40.0]);
}

#[test]
fn auto_width_table_is_as_wide_as_its_content() {
    let widths = widths(
        &format!(
            "<table><tr><td id=a>{}</td><td id=b>{}</td></tr></table>",
            boxes(1, 30),
            boxes(1, 90)
        ),
        &["a", "b"],
    );
    assert_close(&widths, &[30.0, 90.0]);
}

#[test]
fn table_narrower_than_its_content_is_widened() {
    // `width=100` with a 150px box: the table is as wide as its columns'
    // min-content widths, and the box does not overflow its cell.
    let widths = widths(
        &format!(
            "<table width=100><tr><td id=a>{}</td><td id=b>{}</td></tr></table>",
            boxes(1, 150),
            boxes(1, 20)
        ),
        &["a", "b"],
    );
    assert_close(&widths, &[150.0, 20.0]);
}

#[test]
fn fixed_width_cells_of_a_nested_table_shrink_to_fit_their_column() {
    // A cell's `width` sets its column's width but not its min-content
    // width, so the nested table shrinks to the 40% column.
    let widths = widths(
        "<table width=400><tr><td id=a width='40%'>\
         <table><tr><td style='width:300px'></td></tr></table></td>\
         <td id=b width='60%'></td></tr></table>",
        &["a", "b"],
    );
    assert_close(&widths, &[160.0, 240.0]);
}
