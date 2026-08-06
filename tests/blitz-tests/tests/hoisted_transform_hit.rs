//! Hit testing must find a hoisted child where its transform paints it.
//!
//! A positioned child with a z-index is hoisted up to its stacking context
//! for painting, and hit testing only descends into hoisted children when
//! the point lies inside the recorded content area. A transform moves where
//! the child paints and hit-tests, so the area must cover the transformed
//! box, or a click on the child falls through to whatever it visually
//! covers - a menu positioned with a translate, for example, becomes
//! unclickable.

use blitz_test_harness::{Harness, HarnessOptions};

/// A 60x40 child laid out at the stacking context's origin, painted at
/// (200, 120) by its transform. Both positions stay inside the stacking
/// context's own 300x200 box, so only the content area decides whether
/// the child is found.
///
/// The tests pump twice: content areas are computed from the previous
/// pass's layout and transforms, so they settle one pass after the
/// document is built.
fn harness() -> Harness {
    let html = "<html><body style='margin: 0'>\
         <div id=sc style='position: relative; z-index: 0; width: 300px; height: 200px'>\
         <div id=moved style='position: absolute; left: 0; top: 0; width: 60px; height: 40px; \
         z-index: 1; transform: translate(200px, 120px)'></div>\
         </div></body></html>";
    Harness::from_html_with(
        html,
        HarnessOptions {
            width: 400,
            height: 300,
            ..Default::default()
        },
    )
}

#[test]
fn a_transformed_hoisted_child_is_hit_where_it_paints() {
    let mut harness = harness();
    harness.pump();
    assert_eq!(
        harness.hit_node(230.0, 140.0),
        harness.node("#moved"),
        "the click fell through to what the child visually covers"
    );
}

#[test]
fn the_vacated_layout_position_is_not_hit() {
    let mut harness = harness();
    harness.pump();
    assert_eq!(harness.hit_node(30.0, 20.0), harness.node("#sc"));
}
