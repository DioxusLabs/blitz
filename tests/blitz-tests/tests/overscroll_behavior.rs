//! `overscroll-behavior` stops scroll chaining: the part of a wheel scroll an inner scroller
//! can't consume goes to the page behind it unless the scroller's `overscroll-behavior` on
//! that axis is `contain` or `none`.

use blitz_test_harness::Harness;

/// A 100px square scroller holding a 1000px square child, on a page that scrolls both ways.
fn page(scroller_style: &str) -> Harness {
    Harness::from_html(&format!(
        r#"<html><body style="margin:0; width:3000px; height:3000px">
            <div id="scroller" style="width:100px; height:100px; overflow:auto; {scroller_style}">
                <div style="width:1000px; height:1000px"></div>
            </div>
        </body></html>"#
    ))
}

/// Scroll the scroller to its end on both axes, then keep scrolling, returning the page's
/// scroll offset.
fn overscroll(harness: &mut Harness) -> (f64, f64) {
    harness.wheel_at(50.0, 50.0, -2000.0, -2000.0);
    let scroller = harness.node("#scroller");
    let offset = *harness.base().get_node(scroller).unwrap().scroll_offset();
    assert_eq!((offset.x, offset.y), (900.0, 900.0));
    harness.wheel_at(50.0, 50.0, -100.0, -100.0);
    let page = harness.base().viewport_scroll();
    (page.x, page.y)
}

#[test]
fn auto_chains_to_the_page() {
    assert_eq!(overscroll(&mut page("")), (1200.0, 1200.0));
}

#[test]
fn contain_keeps_the_scroll() {
    assert_eq!(
        overscroll(&mut page("overscroll-behavior: contain")),
        (0.0, 0.0)
    );
}

#[test]
fn none_keeps_the_scroll() {
    assert_eq!(
        overscroll(&mut page("overscroll-behavior: none")),
        (0.0, 0.0)
    );
}

#[test]
fn applies_per_axis() {
    assert_eq!(
        overscroll(&mut page("overscroll-behavior-y: contain")),
        (1200.0, 0.0)
    );
}

#[test]
fn ignored_on_a_box_that_is_not_a_scroll_container() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0; height:3000px">
            <div style="overscroll-behavior: contain">
                <div id="target" style="height:100px"></div>
            </div>
        </body></html>"#,
    );
    harness.wheel_at(50.0, 50.0, 0.0, -100.0);
    assert_eq!(harness.base().viewport_scroll().y, 100.0);
}

#[test]
fn applies_to_text_inputs() {
    let text = "line\n".repeat(50);
    let mut harness = Harness::from_html(&format!(
        r#"<html><body style="margin:0; height:3000px">
            <textarea id="input" style="display:block; width:100px; height:100px; overscroll-behavior:contain">{text}</textarea>
        </body></html>"#
    ));
    harness.wheel_at(50.0, 50.0, 0.0, -5000.0);
    harness.wheel_at(50.0, 50.0, 0.0, -100.0);
    assert_eq!(harness.base().viewport_scroll().y, 0.0);
}
