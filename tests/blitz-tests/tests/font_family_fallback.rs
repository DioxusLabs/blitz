//! When none of the families in a `font-family` list is available, `ch` and the
//! other font-relative units resolve against the default font (the initial
//! `serif`), as the text itself does.

use blitz_test_harness::Harness;

#[test]
fn ch_of_unavailable_families_uses_the_default_font() {
    let harness = Harness::from_html(
        "<html><body style='font-size: 20px'>\
         <div id=missing style='font-family: \"No Such Family\"; width: 10ch; height: 1px'></div>\
         <div id=serif style='font-family: serif; width: 10ch; height: 1px'></div>\
         </body></html>",
    );
    let width = |selector| harness.layout_rect(selector).width;
    assert_eq!(width("#missing"), width("#serif"));
}
