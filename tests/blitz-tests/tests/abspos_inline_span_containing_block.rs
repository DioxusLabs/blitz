//! Out-of-flow boxes whose containing block is an inline span (which has no layout node) are
//! positioned against the padding box of the span's inline root, rather than against a further
//! ancestor. The span's own geometry is not resolved.

use blitz_test_harness::{Harness, Rect};

const STYLE: &str = r#"
    body { margin: 0; font-size: 16px; line-height: 20px; }
    #root { margin: 30px 0 0 40px; width: 400px; height: 60px; padding: 10px; border: 5px solid; }
    .rel { position: relative; }
    .fill { top: 0; left: 0; right: 0; bottom: 0; }
"#;

/// The padding box of `#root`
const ROOT_PADDING_BOX: Rect = Rect {
    x: 45.0,
    y: 35.0,
    width: 420.0,
    height: 80.0,
};

fn layout_rect(body: &str, selector: &str) -> Rect {
    let html = format!("<style>{STYLE}</style><div id='root'>{body}</div>");
    Harness::from_html(&html).layout_rect(selector)
}

#[track_caller]
fn assert_rect_eq(actual: Rect, expected: Rect) {
    let close = |a: f32, b: f32| (a - b).abs() < 1.0;
    assert!(
        close(actual.x, expected.x)
            && close(actual.y, expected.y)
            && close(actual.width, expected.width)
            && close(actual.height, expected.height),
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn child_of_span() {
    let rect = layout_rect(
        "before <span class='rel'>text<div id='abs' class='fill' style='position: absolute'></div></span>",
        "#abs",
    );
    assert_rect_eq(rect, ROOT_PADDING_BOX);
}

#[test]
fn child_of_nested_span() {
    let rect = layout_rect(
        "before <span class='rel'><b>text<div id='abs' class='fill' style='position: absolute'></div></b></span>",
        "#abs",
    );
    assert_rect_eq(rect, ROOT_PADDING_BOX);
}

#[test]
fn descendant_of_inline_box_in_span() {
    let rect = layout_rect(
        "before <span class='rel'><span style='display: inline-block'><p>text</p>\
         <div id='abs' class='fill' style='position: absolute'></div></span></span>",
        "#abs",
    );
    assert_rect_eq(rect, ROOT_PADDING_BOX);
}

#[test]
fn auto_insets_use_static_position() {
    let in_span = layout_rect(
        "before <span class='rel'>text<span id='abs' style='position: absolute'>abs</span></span>",
        "#abs",
    );
    let in_root = layout_rect(
        "before <span>text<span id='abs' style='position: absolute'>abs</span></span>",
        "#abs",
    );
    assert!(
        in_root.x > 55.0,
        "not at the start of the line: {in_root:?}"
    );
    assert_rect_eq(in_span, in_root);
}

#[test]
fn span_which_is_not_a_containing_block() {
    let rect = layout_rect(
        "before <span>text<div id='abs' class='fill' style='position: absolute'></div></span>",
        "#abs",
    );
    assert_eq!((rect.x, rect.y), (0.0, 0.0));
    assert!(rect.width > ROOT_PADDING_BOX.width);
}

#[test]
fn fixed_child_of_relative_span() {
    let rect = layout_rect(
        "before <span class='rel'>text<div id='fixed' class='fill' style='position: fixed'></div></span>",
        "#fixed",
    );
    assert_eq!((rect.x, rect.y), (0.0, 0.0));
    assert!(rect.width > ROOT_PADDING_BOX.width);
}

#[test]
fn fixed_child_of_filtered_span() {
    let rect = layout_rect(
        "before <span style='filter: blur(1px)'>text<div id='fixed' class='fill' style='position: fixed'></div></span>",
        "#fixed",
    );
    assert_rect_eq(rect, ROOT_PADDING_BOX);
}

#[test]
fn fixed_descendant_of_absolute_child_of_relative_span() {
    let rect = layout_rect(
        "before <span class='rel'>text<div style='position: absolute'>\
         <div id='fixed' class='fill' style='position: fixed'></div></div></span>",
        "#fixed",
    );
    assert_eq!((rect.x, rect.y), (0.0, 0.0));
    assert!(rect.width > ROOT_PADDING_BOX.width);
}

#[test]
fn fixed_child_of_transformed_span() {
    // Transforms and containment do not apply to non-atomic inline boxes
    let rect = layout_rect(
        "before <span style='transform: scale(1); will-change: transform; contain: paint'>text\
         <div id='fixed' class='fill' style='position: fixed'></div></span>",
        "#fixed",
    );
    assert_eq!((rect.x, rect.y), (0.0, 0.0));
    assert!(rect.width > ROOT_PADDING_BOX.width);
}

#[test]
fn fixed_descendant_unclaimed_by_span_is_claimed_by_root() {
    let html = format!(
        "<style>{STYLE} #root {{ filter: blur(1px) }}</style><div id='root'>before \
         <span class='rel'>text<div style='position: absolute'>\
         <div id='fixed' class='fill' style='position: fixed'></div></div></span></div>"
    );
    let rect = Harness::from_html(&html).layout_rect("#fixed");
    assert_rect_eq(rect, ROOT_PADDING_BOX);
}

/// The root's claims are the union of its spans', so a box beside a containing-block span is
/// claimed by the root too (imprecise: its containing block is really a further ancestor)
#[test]
fn sibling_of_span_with_inline_box_is_claimed_by_root() {
    let rect = layout_rect(
        "<span class='rel'><span style='display: inline-block'>box</span></span>\
         <div id='abs' class='fill' style='position: absolute'></div>",
        "#abs",
    );
    assert_rect_eq(rect, ROOT_PADDING_BOX);
}

/// A containing-block span with no inline box below it does not widen the root's claims
#[test]
fn sibling_of_text_only_span_is_not_claimed_by_root() {
    let rect = layout_rect(
        "<span class='rel'>text</span>\
         <div id='abs' class='fill' style='position: absolute'></div>",
        "#abs",
    );
    assert_rect_eq(
        rect,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        },
    );
}

/// Adding or removing a containing-block-establishing property on a span updates the root's
/// claims
fn assert_span_cb_toggle(cb_prop: &str, position: &str) {
    use markup5ever::{QualName, local_name, ns};

    let html = format!(
        "<style>{STYLE}</style><div id='root'>before <span id='span'>text\
         <div id='abs' class='fill' style='position: {position}'></div></span></div>"
    );
    let mut harness = Harness::from_html(&html);
    let span = harness.node("#span");
    let viewport = harness.layout_rect("#abs");
    assert!(
        viewport.width > ROOT_PADDING_BOX.width,
        "initially unclaimed"
    );

    let mut set_style = |harness: &mut Harness, style: &str| {
        let name = QualName::new(None, ns!(), local_name!("style"));
        harness.base_mut().mutate().set_attribute(span, name, style);
        harness.pump();
    };

    set_style(&mut harness, cb_prop);
    assert_rect_eq(harness.layout_rect("#abs"), ROOT_PADDING_BOX);

    set_style(&mut harness, "");
    assert_rect_eq(harness.layout_rect("#abs"), viewport);
}

#[test]
fn dynamic_position_on_span() {
    assert_span_cb_toggle("position: relative", "absolute");
}

#[test]
fn dynamic_filter_on_span() {
    assert_span_cb_toggle("filter: blur(1px)", "fixed");
}

#[test]
fn dynamic_will_change_on_span() {
    assert_span_cb_toggle("will-change: filter", "fixed");
    assert_span_cb_toggle("will-change: position", "absolute");
}

/// As `assert_span_cb_toggle`, but through `:hover`, which damages the span through its style
/// change alone
fn assert_span_cb_hover_toggle(cb_prop: &str, position: &str) {
    let html = format!(
        "<style>{STYLE} #span:hover {{ {cb_prop} }}</style><div id='root'><span id='span'>text\
         <div id='abs' class='fill' style='position: {position}'></div></span></div>"
    );
    let mut harness = Harness::from_html(&html);
    let viewport = harness.layout_rect("#abs");
    assert!(
        viewport.width > ROOT_PADDING_BOX.width,
        "initially unclaimed"
    );

    harness.base_mut().set_hover_to(60.0, 50.0);
    harness.pump();
    assert_rect_eq(harness.layout_rect("#abs"), ROOT_PADDING_BOX);

    harness.base_mut().set_hover_to(700.0, 500.0);
    harness.pump();
    assert_rect_eq(harness.layout_rect("#abs"), viewport);
}

#[test]
fn hover_position_on_span() {
    assert_span_cb_hover_toggle("position: relative", "absolute");
}

#[test]
fn hover_filter_on_span() {
    assert_span_cb_hover_toggle("filter: blur(1px)", "fixed");
}

#[test]
fn hover_will_change_on_span() {
    assert_span_cb_hover_toggle("will-change: filter", "fixed");
    assert_span_cb_hover_toggle("will-change: position", "absolute");
}
