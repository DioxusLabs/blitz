//! When a child has a CSS transform that moves it outside its parent's
//! `overflow: hidden` clip region, the child is visually clipped and must
//! not be hit-tested. Previously, `scrollable_overflow` extended beyond
//! the border box, so hit testing matched the visually-clipped child,
//! causing incorrect cursor and hover state.

use blitz_test_harness::{Harness, HarnessOptions};

fn harness(html: &str) -> Harness {
    Harness::from_html_with(
        html,
        HarnessOptions {
            width: 400,
            height: 400,
            ..Default::default()
        },
    )
}

#[test]
fn overflow_hidden_clips_transformed_child_from_hit_testing() {
    // The parent is a 100×100 box with overflow:hidden at (0,0).
    // The child is 50×50, positioned at (0,0) but translated 200px right,
    // so it paints at (200,0) -- far outside the parent's clip region.
    // A click at (200, 25) should NOT hit the child.
    let harness = harness(
        r#"<html><body style="margin:0">
        <div id="parent" style="overflow:hidden; width:100px; height:100px;">
            <div id="child" style="width:50px; height:50px; transform:translateX(200px);"></div>
        </div>
        <div id="sibling" style="width:100px; height:100px; margin-top:50px;"></div>
    </body></html>"#,
    );

    let parent = harness.node("#parent");
    let child = harness.node("#child");

    // (50, 50) is inside the parent's clip region but where the child would
    // be *without* the transform. With the transform, the child is at
    // (200..250, 0..50). (50, 50) should not hit the child.
    let hit = harness.hit(50.0, 50.0);
    if let Some(hit) = hit {
        assert_ne!(
            hit.node_id, child,
            "hit at (50, 50) should not match the transformed-away child"
        );
    }

    // (210, 25) is where the child visually appears, but it is outside the
    // parent's overflow:hidden clip, so it must not be hit-tested.
    let hit = harness.hit(210.0, 25.0);
    if let Some(hit) = hit {
        assert_ne!(
            hit.node_id, child,
            "hit at (210, 25) should not match the clipped child"
        );
        assert_ne!(
            hit.node_id, parent,
            "hit at (210, 25) should not match the parent (outside clip)"
        );
    }
}

#[test]
fn overflow_visible_does_not_clip_children() {
    // Same layout but with overflow:visible -- the child is not clipped
    // and should be hit-testable at its transformed position.
    let harness = harness(
        r#"<html><body style="margin:0">
        <div id="parent" style="overflow:visible; width:100px; height:100px;">
            <div id="child" style="width:50px; height:50px; transform:translateX(200px);"></div>
        </div>
    </body></html>"#,
    );

    let child = harness.node("#child");

    // (210, 25) is where the child visually appears with overflow:visible.
    let hit = harness.hit(210.0, 25.0);
    assert!(
        hit.is_some(),
        "expected a hit at (210, 25) with overflow:visible"
    );
    let hit = hit.unwrap();
    assert_eq!(
        hit.node_id, child,
        "hit at (210, 25) should match the child when overflow is visible"
    );
}

#[test]
fn overflow_hidden_child_inside_clip_region_is_still_hittable() {
    // The child is inside the parent's clip region, so it should be
    // hit-testable normally.
    let harness = harness(
        r#"<html><body style="margin:0">
        <div id="parent" style="overflow:hidden; width:200px; height:200px;">
            <div id="child" style="width:50px; height:50px; margin:25px;"></div>
        </div>
    </body></html>"#,
    );

    let child = harness.node("#child");

    // (50, 50) is inside the child's box (at 25..75, 25..75 within parent).
    let hit = harness.hit(50.0, 50.0);
    assert!(
        hit.is_some(),
        "expected a hit at (50, 50) inside the clip region"
    );
    let hit = hit.unwrap();
    assert_eq!(
        hit.node_id, child,
        "hit at (50, 50) should match the child inside the clip region"
    );
}
