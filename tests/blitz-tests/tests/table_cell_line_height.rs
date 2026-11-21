//! A table cell's own `vertical-align` places the cell's content in its row;
//! it does not apply to the text inside the cell.
//!
//! NOTE: This test measures real text, so it needs a usable font. Without the
//! `system-fonts` feature text measures 0x0 and the assertion passes
//! vacuously.

use blitz_test_harness::Harness;

#[test]
fn lines_in_a_middle_aligned_cell_are_as_tall_as_in_a_block() {
    // `middle` (the UA style sheet's value for cells) was applied to the
    // cell's text runs too, which made each line ending in `<br>` taller.
    let lines = "x<br>x<br>x<br>x";
    let harness = Harness::from_html(&format!(
        "<html><body style='margin:0; font: 16px sans-serif'>\
         <table style='border-spacing:0'><tr>\
         <td id=cell style='padding:0; vertical-align:middle'>{lines}</td></tr></table>\
         <div id=block style='display:inline-block'>{lines}</div></body></html>"
    ));
    let cell = harness.layout_rect("#cell").height;
    let block = harness.layout_rect("#block").height;
    assert_eq!(cell, block, "cell {cell}, block {block}");
}
