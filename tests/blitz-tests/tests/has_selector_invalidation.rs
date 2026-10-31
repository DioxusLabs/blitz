//! `:has()` re-matches after the DOM changes (Selectors 4 §4.5).
//!
//! Changes inside a `:has()` argument (an element state, an attribute, or an
//! inserted or removed element) invalidate the anchor through Stylo's relative
//! selector invalidation, so the anchor's style follows without restyling the
//! rest of the document.

use blitz_dom::{LocalName, NodeId, QualName, ns};
use blitz_test_harness::Harness;
use style::color::AbsoluteColor;
use style::properties::ComputedValues;
use style::servo_arc::Arc;

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 128, 0];

fn styles_of(harness: &Harness, selector: &str) -> Arc<ComputedValues> {
    let id = harness.node(selector);
    let doc = harness.base();
    let styles = doc.get_node(id).unwrap().primary_styles().unwrap();
    (*styles).clone()
}

fn background_of(harness: &Harness, selector: &str) -> [u8; 3] {
    let c = styles_of(harness, selector)
        .get_background()
        .background_color
        .resolve_to_absolute(&AbsoluteColor::BLACK)
        .components;
    [c.0, c.1, c.2].map(|v| (v * 255.0).round() as u8)
}

fn page(body: &str) -> Harness {
    Harness::from_html(&format!(
        "<html><head><style>\
         div {{ background-color: rgb(255, 0, 0); width: 100px; height: 20px; }} \
         .a:has(input:checked) {{ background-color: rgb(0, 128, 0); }} \
         .b:has(> .x) {{ background-color: rgb(0, 128, 0); }} \
         .c:has(+ .x) {{ background-color: rgb(0, 128, 0); }} \
         .d:has(.x) {{ background-color: rgb(0, 128, 0); }} \
         </style></head><body>{body}</body></html>"
    ))
}

fn qual_name(name: &str) -> QualName {
    QualName::new(None, ns!(html), LocalName::from(name))
}

fn attr_name(name: &str) -> QualName {
    QualName::new(None, ns!(), LocalName::from(name))
}

fn create_x(harness: &mut Harness, tag: &str) -> NodeId {
    let mut doc = harness.base_mut();
    let mut mutator = doc.mutate();
    let id = mutator.create_element(qual_name(tag), Vec::new());
    mutator.set_attribute(id, attr_name("class"), "x");
    id
}

#[test]
fn has_rematches_after_a_state_change() {
    let mut harness = page(
        "<div id=anchor class=a><input id=box type=checkbox></div>\
         <div id=other></div>",
    );
    assert_eq!(background_of(&harness, "#anchor"), RED);
    let other = styles_of(&harness, "#other");

    harness.click("#box");
    assert_eq!(background_of(&harness, "#anchor"), GREEN);

    harness.click("#box");
    assert_eq!(background_of(&harness, "#anchor"), RED);

    // Only the anchor is invalidated: an unrelated element keeps its style.
    assert!(Arc::ptr_eq(&other, &styles_of(&harness, "#other")));
}

#[test]
fn has_rematches_after_an_attribute_change() {
    let mut harness = page("<div id=anchor class=b><span id=child></span></div>");
    assert_eq!(background_of(&harness, "#anchor"), RED);
    let child = harness.node("#child");

    harness
        .base_mut()
        .mutate()
        .set_attribute(child, attr_name("class"), "x");
    harness.pump();
    assert_eq!(background_of(&harness, "#anchor"), GREEN);

    harness
        .base_mut()
        .mutate()
        .clear_attribute(child, attr_name("class"));
    harness.pump();
    assert_eq!(background_of(&harness, "#anchor"), RED);
}

#[test]
fn has_rematches_after_inserting_and_removing_a_child() {
    let mut harness = page("<div id=anchor class=b><span></span></div><div id=other></div>");
    assert_eq!(background_of(&harness, "#anchor"), RED);
    let other = styles_of(&harness, "#other");
    let anchor = harness.node("#anchor");

    let x = create_x(&mut harness, "span");
    harness.base_mut().mutate().append_children(anchor, &[x]);
    harness.pump();
    assert_eq!(background_of(&harness, "#anchor"), GREEN);

    harness.base_mut().mutate().remove_node(x);
    harness.pump();
    assert_eq!(background_of(&harness, "#anchor"), RED);

    assert!(Arc::ptr_eq(&other, &styles_of(&harness, "#other")));
}

#[test]
fn has_rematches_after_inserting_and_removing_a_descendant() {
    let mut harness = page("<div id=anchor class=d><span><span id=parent></span></span></div>");
    assert_eq!(background_of(&harness, "#anchor"), RED);
    let parent = harness.node("#parent");

    let x = create_x(&mut harness, "span");
    harness.base_mut().mutate().append_children(parent, &[x]);
    harness.pump();
    assert_eq!(background_of(&harness, "#anchor"), GREEN);

    harness.base_mut().mutate().remove_node(x);
    harness.pump();
    assert_eq!(background_of(&harness, "#anchor"), RED);
}

#[test]
fn has_rematches_after_inserting_and_removing_a_sibling() {
    let mut harness = page("<div id=anchor class=c></div><p></p>");
    assert_eq!(background_of(&harness, "#anchor"), RED);
    let anchor = harness.node("#anchor");

    let x = create_x(&mut harness, "p");
    harness.base_mut().mutate().insert_nodes_after(anchor, &[x]);
    harness.pump();
    assert_eq!(background_of(&harness, "#anchor"), GREEN);

    harness.base_mut().mutate().remove_node(x);
    harness.pump();
    assert_eq!(background_of(&harness, "#anchor"), RED);
}

#[test]
fn has_rematches_a_nested_negated_has_after_a_state_change() {
    // hx-native issue #44: a box is ticked only when it holds an icon and no
    // unchecked input.
    let mut harness = Harness::from_html(
        "<html><head><style>\
         .box { background-color: rgb(255, 0, 0); width: 100px; height: 20px; } \
         .ck .box:has(svg):not(:has(input:not(:checked))) { background-color: rgb(0, 128, 0); } \
         </style></head><body><div class=ck><div id=anchor class=box>\
         <input id=box type=checkbox><svg width=10 height=10></svg></div></div></body></html>",
    );
    assert_eq!(background_of(&harness, "#anchor"), RED);

    harness.click("#box");
    assert_eq!(background_of(&harness, "#anchor"), GREEN);

    harness.click("#box");
    assert_eq!(background_of(&harness, "#anchor"), RED);
}
