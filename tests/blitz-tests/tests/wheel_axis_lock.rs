//! A trackpad scroll gesture locks to the axis it mostly moves along, so a slightly crooked
//! vertical swipe over a horizontal scroller doesn't move it sideways. Devices that report no
//! gestures scroll both axes as before.

use blitz_test_harness::Harness;
use blitz_traits::events::BlitzWheelPhase::{self, Ended, Moved, Started};
use blitz_traits::events::BlitzWheelSource::{self, Finger, Momentum, Unknown};

/// A page that scrolls vertically, holding a 100px tall row that scrolls horizontally.
fn page() -> Harness {
    Harness::from_html(
        r#"<html><body style="margin:0; height:3000px">
            <div id="row" style="width:100px; height:100px; overflow-x:auto; overflow-y:hidden">
                <div style="width:1000px; height:100px"></div>
            </div>
        </body></html>"#,
    )
}

/// Send a gesture over the row as `(dx, dy, phase, source)` steps, returning the row's
/// horizontal scroll offset and the page's vertical one.
fn gesture(steps: &[(f64, f64, BlitzWheelPhase, BlitzWheelSource)]) -> (f64, f64) {
    let mut harness = page();
    for &(dx, dy, phase, source) in steps {
        harness.wheel_gesture_at(50.0, 50.0, dx, dy, phase, source);
    }
    let row = harness.node("#row");
    let row_x = harness.base().get_node(row).unwrap().scroll_offset().x;
    (row_x, harness.base().viewport_scroll().y)
}

#[test]
fn a_mostly_vertical_gesture_and_its_momentum_scroll_only_vertically() {
    assert_eq!(
        gesture(&[
            (0.0, 0.0, Started, Finger),
            (-3.0, -30.0, Moved, Finger),
            (-20.0, -30.0, Moved, Finger),
            (-5.0, -10.0, Ended, Finger),
            (0.0, 0.0, Started, Momentum),
            (-5.0, -20.0, Moved, Momentum),
            (0.0, 0.0, Ended, Momentum),
        ]),
        (0.0, 90.0)
    );
}

#[test]
fn a_mostly_horizontal_gesture_scrolls_only_horizontally() {
    assert_eq!(
        gesture(&[
            (-30.0, -5.0, Started, Finger),
            (-30.0, -10.0, Moved, Finger),
        ]),
        (60.0, 0.0)
    );
}

#[test]
fn a_small_first_movement_does_not_decide_the_axis() {
    assert_eq!(
        gesture(&[
            (-1.0, 0.0, Started, Finger),
            (0.0, -30.0, Moved, Finger),
            (-2.0, -30.0, Moved, Finger),
        ]),
        (1.0, 60.0)
    );
}

#[test]
fn a_diagonal_gesture_scrolls_both_axes() {
    assert_eq!(
        gesture(&[
            (0.0, 0.0, Started, Finger),
            (-20.0, -20.0, Moved, Finger),
            (-10.0, 0.0, Moved, Finger),
        ]),
        (30.0, 20.0)
    );
}

#[test]
fn a_gesture_that_turns_changes_its_axis() {
    assert_eq!(
        gesture(&[
            (0.0, -30.0, Started, Finger),
            (-30.0, 0.0, Moved, Finger),
            (-30.0, 0.0, Moved, Finger),
            (-30.0, 0.0, Moved, Finger),
            (-30.0, -5.0, Moved, Finger),
        ]),
        (120.0, 30.0)
    );
}

#[test]
fn each_gesture_decides_its_own_axis() {
    assert_eq!(
        gesture(&[
            (0.0, -30.0, Started, Finger),
            (0.0, 0.0, Ended, Finger),
            (-30.0, 0.0, Started, Finger),
            (-10.0, -10.0, Moved, Finger),
        ]),
        (40.0, 30.0)
    );
}

#[test]
fn events_outside_a_gesture_scroll_both_axes() {
    assert_eq!(
        gesture(&[
            (-3.0, -30.0, Moved, Unknown),
            (-10.0, -10.0, Moved, Unknown),
        ]),
        (13.0, 40.0)
    );
}

#[test]
fn events_after_a_gesture_scroll_both_axes() {
    assert_eq!(
        gesture(&[
            (0.0, -30.0, Started, Finger),
            (0.0, 0.0, Ended, Finger),
            (-10.0, -10.0, Moved, Unknown),
        ]),
        (10.0, 40.0)
    );
}
