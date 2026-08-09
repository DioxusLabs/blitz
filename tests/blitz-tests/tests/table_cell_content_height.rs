//! A table row is as tall as its tallest cell's contents, whatever else in the
//! row specifies a height.
//!
//! A cell whose width is already known is measured width-only first, and that
//! measurement reports no height. It must not stand in for the cell's real
//! height afterwards, or the row collapses onto whatever height is specified
//! elsewhere in it and the text spills out of its cell.

use blitz_test_harness::{Harness, HarnessOptions};

/// The content cell holds a 20px line box and 10px of vertical padding, so it
/// needs 30px whatever the sibling cell says.
fn table(cell_style: &str, sibling_style: &str) -> Harness {
    let html = format!(
        "<html><head><style>body {{ margin: 0; font-size: 14px; line-height: 20px }}\
         </style></head><body>\
         <div style='display: table'><div style='display: table-row'>\
         <div id=content style='display: table-cell; padding: 5px 10px; {cell_style}'>\
         <div id=inner>Entry 0</div></div>\
         <div id=sibling style='display: table-cell; {sibling_style}'></div>\
         </div></div></body></html>"
    );
    Harness::from_html_with(
        &html,
        HarnessOptions {
            width: 600,
            height: 400,
            ..Default::default()
        },
    )
}

#[test]
fn a_cell_with_a_definite_width_still_contributes_its_content_height() {
    let harness = table("width: 256px", "");
    assert_eq!(harness.layout_rect("#content").height, 30.0);
    assert_eq!(harness.layout_rect("#inner").height, 20.0);
}

/// What a data table widget commonly does: every row carries a zero-width spacer cell with
/// the row height it wants. A specified height is a minimum, so the taller
/// content wins.
///
/// The spacer keeps its own specified height rather than stretching to the
/// row, which a browser would do. That is invisible for a zero-width cell, so
/// this only pins down the cell that holds the text.
#[test]
fn a_shorter_specified_height_elsewhere_in_the_row_does_not_win() {
    let harness = table("width: 256px", "vertical-align: top; height: 22px");
    assert_eq!(harness.layout_rect("#content").height, 30.0);
}

/// A taller specified height still stretches the row.
#[test]
fn a_taller_specified_height_stretches_the_row() {
    let harness = table("width: 256px", "height: 50px");
    assert_eq!(harness.layout_rect("#content").height, 50.0);
}

/// The content of a cell laid out at its content-based width was always
/// accounted for; keep it that way.
#[test]
fn a_cell_with_an_automatic_width_is_unchanged() {
    let harness = table("", "vertical-align: top; height: 22px");
    assert_eq!(harness.layout_rect("#content").height, 30.0);
}

/// A cell's specified height is a content-box height, so a spacer that also
/// has padding asks for more of the row than its `height` alone suggests.
#[test]
fn a_spacer_cells_own_padding_counts_toward_the_row() {
    // 22px of content plus 10px of padding beats the 30px content cell
    let harness = table("width: 256px", "height: 22px; padding: 5px 10px");
    assert_eq!(harness.layout_rect("#content").height, 32.0);

    // ... unless the spacer measures its height as a border box
    let harness = table(
        "width: 256px",
        "height: 22px; padding: 5px 10px; box-sizing: border-box",
    );
    assert_eq!(harness.layout_rect("#content").height, 30.0);
}

/// The same shape as a data table widget: fixed column widths from a leading
/// sizing row, and an unpadded spacer cell carrying the row height.
#[test]
fn a_datatable_shaped_row_is_as_tall_as_its_text() {
    let html = "<html><head><style>body { margin: 0; font-size: 14px; line-height: 20px }\
         .content { display: table; table-layout: fixed; width: 1px; border-collapse: collapse }\
         .content .datatable-cell { padding: 5px 10px }\
         .content td { overflow: hidden; white-space: nowrap }\
         </style></head><body>\
         <table class=content>\
         <tr><td style='width: 256px; height: 0px'></td><td style='width: 242px; height: 0px'></td>\
             <td style='width: 0px; height: 0px'></td></tr>\
         <tr><td id=cell class=datatable-cell><div>Entry 0</div></td>\
             <td class=datatable-cell><div>Value 0</div></td>\
             <td style='vertical-align: top; height: 22px'></td></tr>\
         </table></body></html>";
    let harness = Harness::from_html_with(
        html,
        HarnessOptions {
            width: 600,
            height: 400,
            ..Default::default()
        },
    );
    assert_eq!(harness.layout_rect("#cell").height, 30.0);
}
