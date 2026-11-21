//! The `align` attribute of embedded content (`img`, `iframe`, `object`,
//! `embed`, `<input type=image>`) floats it or sets its `vertical-align`.
//! Expected values are what Chromium gives for the same HTML.

use blitz_test_harness::Harness;

const SVG: &str =
    "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%2F%3E";

/// A 10px inline block sitting on the baseline, then a 40px image with the
/// given `align` attribute and extra style. Returns the inline block's top:
/// its bottom is the baseline.
fn marker_top(align: &str, style: &str) -> f32 {
    let align = if align.is_empty() {
        String::new()
    } else {
        format!("align={align}")
    };
    let harness = Harness::from_html(&format!(
        "<html><body style='margin:0'><div style='font:20px/1 monospace'>\
         <span id=m style='display:inline-block; width:10px; height:10px'></span>\
         <img style='width:40px; height:40px; {style}' {align} src=\"{SVG}\"></div>\
         </body></html>"
    ));
    harness.layout_rect("#m").y
}

#[test]
fn align_middle_puts_the_image_middle_on_the_baseline() {
    // The image is 40px tall and the line starts at its top, so the
    // baseline is 20px down and the marker's top is at 10px.
    assert_eq!(marker_top("middle", ""), 10.0);
    // Keywords are ASCII case-insensitive; `center` is the same as `middle`.
    assert_eq!(marker_top("MIDDLE", ""), 10.0);
    assert_eq!(marker_top("center", ""), 10.0);
    // Without `align` the image sits on the baseline.
    assert_eq!(marker_top("", ""), 30.0);
}

#[test]
fn align_maps_to_vertical_align() {
    let va = |value: &str| marker_top("", &format!("vertical-align:{value}"));
    // `absmiddle` is `vertical-align: middle`, which adds half the x-height:
    // the baseline is lower than with `middle`.
    assert_eq!(marker_top("absmiddle", ""), va("middle"));
    assert!(marker_top("absmiddle", "") > 10.0);
    assert_eq!(marker_top("abscenter", ""), va("middle"));
    assert_eq!(marker_top("top", ""), va("top"));
    assert_ne!(va("top"), va("baseline"));
    assert_eq!(marker_top("texttop", ""), va("text-top"));
    assert_eq!(marker_top("absbottom", ""), va("bottom"));
    // `bottom` is the initial `baseline`.
    assert_eq!(marker_top("bottom", ""), 30.0);
}

#[test]
fn author_vertical_align_overrides_align_middle() {
    assert_eq!(
        marker_top("middle", "vertical-align:top"),
        marker_top("", "vertical-align:top")
    );
}

#[test]
fn align_left_floats_the_image() {
    let harness = Harness::from_html(&format!(
        "<html><body style='margin:0'><div style='width:200px'>\
         <img style='width:40px; height:40px' align=left src=\"{SVG}\">\
         <div id=b style='display:flow-root; height:10px'></div></div></body></html>"
    ));
    let b = harness.layout_rect("#b");
    assert_eq!((b.x, b.width), (40.0, 160.0));
}
