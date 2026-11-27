//! A `<br>` between text runs in a flex or grid container breaks the line:
//! it joins the anonymous block the text before it is in, as in Chromium and
//! Firefox, instead of becoming a flex item of its own.
//! Expected heights are what both browsers give for the same HTML.

use blitz_test_harness::Harness;

/// The height of `#c`, laid out with 20px lines.
fn height(container_style: &str, content: &str) -> f32 {
    let harness = Harness::from_html(&format!(
        "<html><body style='margin:0; font:16px/20px sans-serif'>\
         <div id=c style='width:300px; {container_style}'>{content}</div>\
         </body></html>"
    ));
    harness.layout_rect("#c").height
}

#[test]
fn br_between_text_breaks_the_line_in_a_flex_container() {
    assert_eq!(height("display:flex", "first line<br>second line"), 40.0);
    // Whitespace around the `<br>` changes nothing.
    assert_eq!(height("display:flex", "first line <br> second line"), 40.0);
    assert_eq!(height("display:flex", "one<br>two<br>three"), 60.0);
}

#[test]
fn br_between_text_breaks_the_line_in_a_grid_container() {
    // One grid item with two lines: with the `<br>` as an item of its own,
    // each piece would get its own row, with a gap between them.
    assert_eq!(
        height("display:grid; row-gap:10px", "first line<br>second line"),
        40.0
    );
}

#[test]
fn br_between_text_hoisted_from_display_contents_breaks_the_line() {
    assert_eq!(
        height(
            "display:flex",
            "<span style='display:contents'>first line<br>second line</span>"
        ),
        40.0
    );
}

#[test]
fn trailing_br_adds_no_line() {
    assert_eq!(height("display:flex", "first line<br>"), 20.0);
}

#[test]
fn br_between_flex_items_adds_no_line() {
    assert_eq!(
        height("display:flex", "<span>a</span><br><span>b</span>"),
        20.0
    );
}
