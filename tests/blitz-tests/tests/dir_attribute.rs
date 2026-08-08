//! The dir attribute sets the CSS direction, as the user agent stylesheet
//! required by the HTML spec's bidi rendering section maps it.
//!
//! The engine's stylesheet inherited Gecko's bidi rules, which match through
//! Gecko-internal pseudo-classes (`:-moz-dir-attr-rtl` and friends) that the
//! servo selector parser drops, so the attribute set no direction at all.

use blitz_test_harness::{Harness, HarnessOptions};

/// A flex row with two 100px children in a 600px viewport. Under `rtl` the
/// row starts at the right edge, so the first child sits at x = 500.
fn page(html_attrs: &str, inner: &str) -> Harness {
    let html = format!(
        "<html {html_attrs}><head><style>body {{ margin: 0 }}</style></head><body>\
         {inner}\
         <div id=row style='display: flex'>\
         <div id=first style='width: 100px; height: 20px'></div>\
         <div id=second style='width: 100px; height: 20px'></div>\
         </div></body></html>"
    );
    Harness::from_html_with(
        &html,
        HarnessOptions {
            width: 600,
            height: 400,
            ..Default::default()
        },
    )
}

#[test]
fn without_the_attribute_the_row_starts_at_the_left() {
    let harness = page("", "");
    assert_eq!(harness.layout_rect("#first").x, 0.0);
    assert_eq!(harness.layout_rect("#second").x, 100.0);
}

#[test]
fn dir_rtl_starts_the_row_at_the_right() {
    let harness = page("dir=rtl", "");
    assert_eq!(harness.layout_rect("#first").x, 500.0);
    assert_eq!(harness.layout_rect("#second").x, 400.0);
}

/// The attribute value is matched case-insensitively.
#[test]
fn the_attribute_value_is_case_insensitive() {
    let harness = page("dir=RTL", "");
    assert_eq!(harness.layout_rect("#first").x, 500.0);
}

/// An inner dir=ltr island keeps its own direction inside an rtl page.
#[test]
fn an_inner_island_keeps_its_own_direction() {
    let harness = page(
        "dir=rtl",
        "<div dir=ltr style='display: flex'>\
         <div id=island style='width: 100px; height: 20px'></div>\
         </div>",
    );
    assert_eq!(harness.layout_rect("#island").x, 0.0);
    assert_eq!(harness.layout_rect("#first").x, 500.0);
}

/// Toggling the attribute at runtime restyles and relayouts, which is what
/// an application's RTL switcher does.
#[test]
fn toggling_the_attribute_at_runtime_flips_the_layout() {
    let mut harness = page("", "");
    assert_eq!(harness.layout_rect("#first").x, 0.0);

    let root = harness.node("html");
    harness.base_mut().mutate().set_attribute(
        root,
        blitz_dom::QualName::new(None, blitz_dom::ns!(), blitz_dom::local_name!("dir")),
        "rtl",
    );
    harness.pump();
    assert_eq!(harness.layout_rect("#first").x, 500.0);

    harness.base_mut().mutate().clear_attribute(
        root,
        blitz_dom::QualName::new(None, blitz_dom::ns!(), blitz_dom::local_name!("dir")),
    );
    harness.pump();
    assert_eq!(harness.layout_rect("#first").x, 0.0);
}
