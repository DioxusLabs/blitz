//! The CSS `line-break` property controls the soft wrap opportunities used for
//! inline layout.

use blitz_dom::{BaseDocument, node::TextLayout};
use blitz_test_harness::Harness;

fn inline<'a>(doc: &'a BaseDocument, selector: &str) -> &'a TextLayout {
    let id = doc.query_selector(selector).unwrap().unwrap();
    doc.get_node(id)
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .unwrap()
}

fn line_count(line_break: &str, text: &str) -> usize {
    let harness = Harness::from_html(&format!(
        "<div id='test' style='width:1px;line-break:{line_break}'>{text}</div>",
    ));
    let doc = harness.base();
    inline(&doc, "#test").layout.len()
}

#[test]
fn strictness_controls_breaks_before_small_kana() {
    // U+3041 HIRAGANA LETTER SMALL A may only start a line under `normal` and `loose`.
    for (line_break, lines) in [("auto", 2), ("strict", 2), ("normal", 3), ("loose", 3)] {
        assert_eq!(line_count(line_break, "文ぁ文"), lines, "{line_break}");
    }
}

#[test]
fn anywhere_breaks_between_letters() {
    assert_eq!(line_count("normal", "abc"), 1);
    assert_eq!(line_count("anywhere", "abc"), 3);
}

#[test]
fn changing_the_property_relayouts() {
    let mut harness = Harness::from_html("<div id='test' style='width:1px'>abc</div>");
    assert_eq!(inline(&harness.base(), "#test").layout.len(), 1);

    let id = harness.node("#test");
    harness
        .base_mut()
        .mutate()
        .set_style_property(id, "line-break", "anywhere");
    harness.pump();
    assert_eq!(inline(&harness.base(), "#test").layout.len(), 3);
}
