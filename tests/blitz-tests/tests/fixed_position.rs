//! A fixed box the viewport contains stays put when the page scrolls, and is
//! hit where it is painted.

use blitz_test_harness::{Harness, HarnessOptions};

fn viewport() -> HarnessOptions {
    HarnessOptions {
        width: 800,
        height: 600,
        ..Default::default()
    }
}

/// The fixed box sits inside a container that is itself offset down the page,
/// and the page is tall enough to scroll.
fn page(fixed_style: &str) -> String {
    format!(
        "<html><head><style>body {{ margin: 0 }}</style></head><body>\
         <div style='height: 400px'></div>\
         <div id=host style='height: 200px'>\
         <div id=t style='width: 100px; height: 50px; {fixed_style}'></div>\
         </div>\
         <div style='height: 3000px'></div></body></html>"
    )
}

fn harness(fixed_style: &str) -> Harness {
    Harness::from_html_with(&page(fixed_style), viewport())
}

/// Hit testing finds it where it is painted, at the same client position,
/// whatever the page scroll is.
#[test]
fn hit_testing_finds_a_fixed_box_where_it_is_painted() {
    let mut harness = harness("position: fixed; top: 10px; left: 20px");
    let node = harness.node("#t");

    // page coordinates equal client coordinates while unscrolled
    assert_eq!(harness.hit(30.0, 20.0).map(|hit| hit.node_id), Some(node));

    harness.base_mut().scroll_viewport_by(0.0, -300.0);
    harness.pump();

    // the box has not moved on screen, so the same client point still hits it:
    // `hit` takes document coordinates, hence the scroll added back on
    assert_eq!(harness.hit(30.0, 320.0).map(|hit| hit.node_id), Some(node));
}

/// An absolutely positioned box in the same place scrolls with the page, so
/// the exemption is not handed out to everything.
#[test]
fn an_absolute_box_is_hit_where_the_page_moved_it() {
    let mut harness = harness("position: absolute; top: 10px; left: 20px");
    let node = harness.node("#t");

    harness.base_mut().scroll_viewport_by(0.0, -300.0);
    harness.pump();

    // it moved up the screen with the page, so it stays at its page position
    assert_eq!(harness.hit(30.0, 20.0).map(|hit| hit.node_id), Some(node));
    assert_ne!(harness.hit(30.0, 320.0).map(|hit| hit.node_id), Some(node));
}
