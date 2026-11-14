//! Wheel scroll over an iframe which its sub-document cannot consume chains to the outer
//! document.

use blitz_test_harness::Harness;

fn harness_with_iframe(iframe_content_height: u32) -> Harness {
    let srcdoc = format!(
        "<body style='margin:0'><div style='height:{iframe_content_height}px'></div></body>"
    );
    let harness = Harness::from_html(&format!(
        r#"<html><body style="margin:0">
            <iframe id="frame" srcdoc="{srcdoc}" style="display:block; width:200px; height:200px; border:0"></iframe>
            <div style="height:2000px"></div>
        </body></html>"#
    ));
    let iframe = harness.node("#frame");
    assert!(harness.base().subdoc(iframe).is_some());
    harness
}

fn iframe_scroll_y(harness: &Harness) -> f64 {
    let doc = harness.base();
    doc.subdoc(harness.node("#frame"))
        .unwrap()
        .inner()
        .viewport_scroll()
        .y
}

#[test]
fn wheel_over_unscrollable_iframe_scrolls_outer_document() {
    let mut harness = harness_with_iframe(50);

    harness.wheel_at(100.0, 100.0, 0.0, -100.0);

    assert_eq!(iframe_scroll_y(&harness), 0.0);
    assert_eq!(harness.base().viewport_scroll().y, 100.0);
}

#[test]
fn wheel_over_iframe_scrolls_iframe_before_outer_document() {
    let mut harness = harness_with_iframe(300);

    harness.wheel_at(100.0, 100.0, 0.0, -60.0);
    assert_eq!(iframe_scroll_y(&harness), 60.0);
    assert_eq!(harness.base().viewport_scroll().y, 0.0);

    harness.wheel_at(100.0, 100.0, 0.0, -60.0);
    assert_eq!(iframe_scroll_y(&harness), 100.0);
    assert_eq!(harness.base().viewport_scroll().y, 20.0);
}
