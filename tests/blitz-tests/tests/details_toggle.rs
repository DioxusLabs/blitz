//! Toggling a `<details>` element open/closed via its `<summary>` must
//! relayout (and reposition for painting/hit-testing) the content that
//! follows it in the same frame, not only once some unrelated event
//! (e.g. a mouse move) happens to trigger another resolve.

use blitz_test_harness::{Harness, HarnessOptions};

const HTML: &str = r#"<html><body style="margin:0">
    <div class="list">
        <details id="d1" open>
            <summary style="display:block; height:20px">One</summary>
            <div class="answer" style="height:100px">Answer one</div>
        </details>
        <details id="d2">
            <summary style="display:block; height:20px">Two</summary>
            <div class="answer" style="height:100px">Answer two</div>
        </details>
    </div>
    <div id="after" style="height:10px"></div>
    <div id="hoisted" style="position:relative; z-index:1; height:10px"></div>
</body></html>"#;

fn harness() -> Harness {
    Harness::from_html_with(
        HTML,
        HarnessOptions {
            width: 400,
            height: 600,
            ..Default::default()
        },
    )
}

#[test]
fn closing_details_shifts_following_content_up() {
    let mut harness = harness();
    assert_eq!(harness.layout_rect("#d2").y, 120.0);
    assert_eq!(harness.layout_rect("#after").y, 140.0);

    harness.click("#d1 summary");
    assert_eq!(harness.attr("#d1", "open"), None);
    assert_eq!(harness.layout_rect("#d1").height, 20.0);
    assert_eq!(harness.layout_rect("#d2").y, 20.0);
    assert_eq!(harness.layout_rect("#after").y, 40.0);
}

#[test]
fn opening_details_shifts_following_content_down() {
    let mut harness = harness();
    harness.click("#d2 summary");
    assert!(harness.attr("#d2", "open").is_some());
    assert_eq!(harness.layout_rect("#d2").height, 120.0);
    assert_eq!(harness.layout_rect("#after").y, 240.0);
}

/// z-indexed boxes are hoisted to their stacking context root for painting
/// and hit-testing, with an offset relative to that root. The offset must be
/// recomputed from the new layout in the same frame as the toggle.
#[test]
fn toggling_details_repositions_hoisted_following_content() {
    let mut harness = harness();
    let hoisted = harness.node("#hoisted");
    assert_eq!(harness.layout_rect("#hoisted").y, 150.0);
    assert_eq!(harness.hit_node(200.0, 155.0), hoisted);

    harness.click("#d1 summary");
    assert_eq!(harness.layout_rect("#hoisted").y, 50.0);
    assert_eq!(harness.hit_node(200.0, 55.0), hoisted);
    assert_ne!(harness.hit(200.0, 155.0).map(|h| h.node_id), Some(hoisted));

    harness.click("#d2 summary");
    assert_eq!(harness.layout_rect("#hoisted").y, 150.0);
    assert_eq!(harness.hit_node(200.0, 155.0), hoisted);
    assert_ne!(harness.hit(200.0, 55.0).map(|h| h.node_id), Some(hoisted));
}
