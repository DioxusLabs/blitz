//! In the collapsing border model, neighbouring cells share one border.

use blitz_test_harness::Harness;

#[test]
fn collapsed_borders_take_one_border_between_rows() {
    // Three 10px rows with 1px borders: four borders and three rows make a
    // 34px table, as in browsers. Counting each cell's border inside the cell
    // as well made every row 2px taller (40px).
    let row = "<tr><td style='border:1px solid black; padding:0'>\
               <div style='height:10px'></div></td></tr>";
    let harness = Harness::from_html(&format!(
        "<html><body style='margin:0'>\
         <table id=t style='border-collapse:collapse; width:100px'>{row}{row}{row}</table>\
         </body></html>"
    ));
    assert_eq!(harness.layout_rect("#t").height, 34.0);
}
