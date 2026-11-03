//! `:has()` is parsed and matches on the initial style resolution.
//!
//! Stylo only parses `:has()` when the `layout.css.has-selector.enabled` pref
//! is set; without it every rule using `:has()` is dropped as invalid.

use blitz_test_harness::Harness;
use style::color::AbsoluteColor;

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 128, 0];

fn background_of(harness: &Harness, selector: &str) -> [u8; 3] {
    let id = harness.node(selector);
    let doc = harness.base();
    let styles = doc.get_node(id).unwrap().primary_styles().unwrap();
    let c = styles
        .get_background()
        .background_color
        .resolve_to_absolute(&AbsoluteColor::BLACK)
        .components;
    [c.0, c.1, c.2].map(|v| (v * 255.0).round() as u8)
}

fn page(body: &str) -> Harness {
    Harness::from_html(&format!(
        "<html><head><style>\
         div {{ background-color: rgb(255, 0, 0); }} \
         .a:has(input:checked) {{ background-color: rgb(0, 128, 0); }} \
         .b:has(> .x) {{ background-color: rgb(0, 128, 0); }} \
         .c:has(+ .x) {{ background-color: rgb(0, 128, 0); }} \
         </style></head><body>{body}</body></html>"
    ))
}

#[test]
fn has_matches_a_descendant_in_a_given_state() {
    let harness = page(
        "<div id=yes class=a><input type=checkbox checked></div>\
         <div id=no class=a><input type=checkbox></div>",
    );
    assert_eq!(background_of(&harness, "#yes"), GREEN);
    assert_eq!(background_of(&harness, "#no"), RED);
}

#[test]
fn has_matches_children_and_siblings() {
    let harness = page(
        "<div id=child class=b><span class=x></span></div>\
         <div id=grandchild class=b><span><span class=x></span></span></div>\
         <div id=sibling class=c></div><p class=x></p>",
    );
    assert_eq!(background_of(&harness, "#child"), GREEN);
    assert_eq!(background_of(&harness, "#grandchild"), RED);
    assert_eq!(background_of(&harness, "#sibling"), GREEN);
}

#[test]
fn has_works_in_query_selector() {
    let harness = page("<div id=yes class=a><input type=checkbox checked></div>");
    assert_eq!(harness.query("div:has(input)"), Some(harness.node("#yes")));
}
