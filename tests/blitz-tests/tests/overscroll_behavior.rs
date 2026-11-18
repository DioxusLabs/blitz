use blitz_test_harness::Harness;

/// Scroll a 100px scroller holding a 1000px child past its end on both axes, returning the
/// page's scroll offset.
fn overscroll(scroller_style: &str) -> (f64, f64) {
    let mut harness = Harness::from_html(&format!(
        r#"<html><body style="margin:0; width:3000px; height:3000px">
            <div style="width:100px; height:100px; overflow:auto; {scroller_style}">
                <div style="width:1000px; height:1000px"></div>
            </div>
        </body></html>"#
    ));
    harness.wheel_at(50.0, 50.0, -1000.0, -1000.0);
    let page = harness.base().viewport_scroll();
    (page.x, page.y)
}

#[test]
fn contain_stops_scroll_chaining() {
    assert_eq!(overscroll(""), (100.0, 100.0));
    assert_eq!(overscroll("overscroll-behavior: contain"), (0.0, 0.0));
}

#[test]
fn overscroll_behavior_applies_per_axis() {
    assert_eq!(overscroll("overscroll-behavior-y: contain"), (100.0, 0.0));
}

#[test]
fn overscroll_behavior_ignored_on_non_scroll_container() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0; height:3000px">
            <div style="overscroll-behavior: contain">
                <div style="height:100px"></div>
            </div>
        </body></html>"#,
    );
    harness.wheel_at(50.0, 50.0, 0.0, -100.0);
    assert_eq!(harness.base().viewport_scroll().y, 100.0);
}
