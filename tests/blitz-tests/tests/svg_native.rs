//! First-party inline `<svg>` (the `svg-native` feature).

use blitz_dom::node::SpecialElementData;
use blitz_test_harness::Harness;

const HTML: &str = r#"<!DOCTYPE html>
<html><body>
<svg id="root" width="100" height="100">
  <rect id="plain" fill="red"></rect>
  <rect id="overridden" fill="red" style="fill: blue"></rect>
  <svg id="nested"></svg>
</svg>
</body></html>
"#;

fn is_svg_root(harness: &Harness, selector: &str) -> bool {
    let node_id = harness.node(selector);
    let doc = harness.base();
    matches!(
        doc.get_node(node_id)
            .unwrap()
            .element_data()
            .unwrap()
            .special_data,
        SpecialElementData::SvgRoot(_)
    )
}

#[test]
fn root_svg_becomes_svg_root() {
    let harness = Harness::from_html(HTML);
    assert!(is_svg_root(&harness, "#root"));
}

/// A nested `<svg>` stays part of the enclosing fragment rather
/// than becoming its own fragment root.
#[test]
fn nested_svg_is_not_its_own_root() {
    let harness = Harness::from_html(HTML);
    assert!(!is_svg_root(&harness, "#nested"));
}

#[test]
fn fragment_is_built_with_the_layout_viewport_size() {
    let harness = Harness::from_html(HTML);
    let root_id = harness.node("#root");
    let doc = harness.base();
    let ctx = doc
        .get_node(root_id)
        .unwrap()
        .element_data()
        .unwrap()
        .svg_root_data()
        .expect("root should have a built SvgContext");
    assert_eq!(ctx.viewport.width, 100.0);
    assert_eq!(ctx.viewport.height, 100.0);
}

#[test]
fn presentation_attribute_reaches_the_cascade() {
    let harness = Harness::from_html(HTML);
    let node_id = harness.node("#plain");
    let fill = harness.base().resolved_style_value(node_id, "fill");
    assert_eq!(fill, "rgb(255, 0, 0)");
}

/// CSS beats the presentation attribute, because both are synthesized
/// at the `PresHints` cascade level that author CSS always outranks.
#[test]
fn author_css_wins_over_the_presentation_attribute() {
    let harness = Harness::from_html(HTML);
    let node_id = harness.node("#overridden");
    let fill = harness.base().resolved_style_value(node_id, "fill");
    assert_eq!(fill, "rgb(0, 0, 255)");
}

/// Construction degrades on malformed input instead of panicking.
#[test]
fn malformed_viewbox_does_not_panic() {
    let html = r#"<!DOCTYPE html>
<html><body><svg id="root" viewBox="not a viewbox"></svg></body></html>
"#;
    let harness = Harness::from_html(html);
    assert!(is_svg_root(&harness, "#root"));
}
