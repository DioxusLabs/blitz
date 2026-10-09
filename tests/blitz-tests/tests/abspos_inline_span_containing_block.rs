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
