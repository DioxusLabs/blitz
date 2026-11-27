//! The client rect of a fixed box the viewport contains stays put when the page
//! scrolls, as the box does on screen.

use blitz_test_harness::{Harness, HarnessOptions};

fn viewport() -> HarnessOptions {
    HarnessOptions {
        width: 800,
        height: 600,
        ..Default::default()
    }
}

/// The box sits inside a container that is itself offset down the page, and
/// the page is tall enough to scroll. It holds a span, which has no box of its
/// own and is reported through its line fragments.
fn page(style: &str) -> String {
    format!(
        "<html><head><style>body {{ margin: 0 }}</style></head><body>\
         <div style='height: 400px'></div>\
         <div id=host style='height: 200px'>\
         <div id=t style='width: 100px; height: 50px; {style}'><span id=s>text</span></div>\
         </div>\
         <div style='height: 3000px'></div></body></html>"
    )
}

/// Scroll the page down by 300px.
fn scroll(harness: &mut Harness) {
    harness.base_mut().scroll_viewport_by(0.0, -300.0);
    harness.pump();
    assert_eq!(
        harness.base().viewport_scroll().y,
        300.0,
        "the page scrolled"
    );
}

#[test]
fn a_fixed_box_does_not_move_with_the_page() {
    let mut harness =
        Harness::from_html_with(&page("position: fixed; top: 10px; left: 20px"), viewport());
    let node = harness.node("#t");
    let before = harness.base().get_client_bounding_rect(node).unwrap();
    assert_eq!((before.x, before.y), (20.0, 10.0));

    scroll(&mut harness);

    let after = harness.base().get_client_bounding_rect(node).unwrap();
    assert_eq!((after.x, after.y), (before.x, before.y));
}

/// The fragments of an inline element inside the box stay with it, and its
/// offset from the box does not change.
#[test]
fn text_in_a_fixed_box_does_not_move_with_the_page() {
    let mut harness =
        Harness::from_html_with(&page("position: fixed; top: 10px; left: 20px"), viewport());
    let span = harness.node("#s");
    let before = harness.base().node_client_rects(span)[0].y;
    let offset_before = harness.base().offset_rect(span).unwrap().y;

    scroll(&mut harness);

    assert_eq!(harness.base().node_client_rects(span)[0].y, before);
    assert_eq!(harness.base().offset_rect(span).unwrap().y, offset_before);
}

/// An absolutely positioned box in the same place keeps scrolling with the
/// page, so the exemption is not handed out to everything.
#[test]
fn an_absolute_box_still_moves_with_the_page() {
    let mut harness = Harness::from_html_with(
        &page("position: absolute; top: 10px; left: 20px"),
        viewport(),
    );
    let node = harness.node("#t");
    let before = harness.base().get_client_bounding_rect(node).unwrap();

    scroll(&mut harness);

    let after = harness.base().get_client_bounding_rect(node).unwrap();
    assert_eq!(after.y, before.y - 300.0);
}
