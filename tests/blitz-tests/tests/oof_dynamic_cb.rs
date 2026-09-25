//! Dynamic containing-block changes: when a style change adds or removes a
//! containing-block-establishing property (transform, will-change, filter,
//! contain, position) on an ancestor, hoisted `position: absolute` / `fixed`
//! descendants must move to their new containing block on the next relayout,
//! even with incremental layout's hot caches.
//!
//! Rust ports of the script-driven WPT tests
//! `css/css-transforms/transform-containing-block-dynamic-1b.html`,
//! `css/filter-effects/filter-cb-dynamic-1b.html`,
//! `css/css-will-change/will-change-abspos-cb-dynamic-001.html` and
//! `css/css-contain/contain-layout-020.html`, which Blitz's WPT runner cannot
//! run (they require script).

use blitz_test_harness::Harness;
use blitz_traits::node_id::NodeId;
use markup5ever::{QualName, local_name, ns};

fn style_attr() -> QualName {
    QualName::new(None, ns!(), local_name!("style"))
}

/// A fixed box nested inside `#anc` (offset 50,50 from the page origin).
/// Without a CB-establishing property on `#anc` the fixed box is positioned
/// against the viewport at (10, 10); with one it is positioned against
/// `#anc` at (60, 60) in page coordinates.
fn fixed_page(anc_style: &str) -> Harness {
    let html = format!(
        "<html><head><style>body {{ margin: 0 }}</style></head><body>\
         <div id=anc style='margin: 50px; width: 300px; height: 300px; {anc_style}'>\
         <div id=spacer style='height: 30px'></div>\
         <div id=target style='position: fixed; top: 10px; left: 10px; width: 20px; height: 20px'></div>\
         </div></body></html>"
    );
    Harness::from_html(&html)
}

fn set_style(harness: &mut Harness, node: NodeId, style: &str) {
    harness
        .base_mut()
        .mutate()
        .set_attribute(node, style_attr(), style);
    harness.pump();
}

const ANC_BASE: &str = "margin: 50px; width: 300px; height: 300px;";

fn assert_fixed_cb_toggle(cb_prop: &str) {
    // Add the CB-establishing property dynamically
    let mut harness = fixed_page("");
    let anc = harness.node("#anc");
    assert_eq!(
        harness.layout_rect("#target").x,
        10.0,
        "initial (viewport CB)"
    );
    assert_eq!(
        harness.layout_rect("#target").y,
        10.0,
        "initial (viewport CB)"
    );

    set_style(&mut harness, anc, &format!("{ANC_BASE} {cb_prop}"));
    assert_eq!(
        harness.layout_rect("#target").x,
        60.0,
        "after adding `{cb_prop}`"
    );
    assert_eq!(
        harness.layout_rect("#target").y,
        60.0,
        "after adding `{cb_prop}`"
    );

    // And remove it again
    set_style(&mut harness, anc, ANC_BASE);
    assert_eq!(
        harness.layout_rect("#target").x,
        10.0,
        "after removing `{cb_prop}`"
    );
    assert_eq!(
        harness.layout_rect("#target").y,
        10.0,
        "after removing `{cb_prop}`"
    );
}

#[test]
fn transform_toggles_fixed_containing_block() {
    assert_fixed_cb_toggle("transform: translateX(0px)");
}

#[test]
fn will_change_toggles_fixed_containing_block() {
    assert_fixed_cb_toggle("will-change: transform");
}

#[test]
fn filter_toggles_fixed_containing_block() {
    assert_fixed_cb_toggle("filter: grayscale(50%)");
}

#[test]
fn contain_toggles_fixed_containing_block() {
    assert_fixed_cb_toggle("contain: layout");
}

/// Toggling `position: relative` on a static intermediate ancestor moves an
/// absolutely positioned descendant between containing blocks.
#[test]
fn position_toggles_absolute_containing_block() {
    let html = "<html><head><style>body { margin: 0 }</style></head><body>\
         <div id=outer style='position: relative; margin: 20px; width: 400px; height: 400px'>\
         <div id=mid style='margin: 30px; width: 300px; height: 300px'>\
         <div id=target style='position: absolute; top: 10px; left: 10px; width: 20px; height: 20px'></div>\
         </div></div></body></html>";
    let mut harness = Harness::from_html(html);
    let mid = harness.node("#mid");

    // CB is #outer at (20, 30): #mid's 30px top margin collapses through #outer
    assert_eq!(harness.layout_rect("#target").x, 30.0);
    assert_eq!(harness.layout_rect("#target").y, 40.0);

    // Make #mid positioned: CB becomes #mid at (50, 30)
    set_style(
        &mut harness,
        mid,
        "position: relative; margin: 30px; width: 300px; height: 300px",
    );
    assert_eq!(harness.layout_rect("#target").x, 60.0);
    assert_eq!(harness.layout_rect("#target").y, 40.0);

    // Back to static: CB is #outer again
    set_style(
        &mut harness,
        mid,
        "margin: 30px; width: 300px; height: 300px",
    );
    assert_eq!(harness.layout_rect("#target").x, 30.0);
    assert_eq!(harness.layout_rect("#target").y, 40.0);
}

/// CB changes driven purely by a restyle (`:hover`), with no DOM mutation. This
/// exercises the pure restyle-damage path (DOM mutations like `set_attribute`
/// insert full damage unconditionally, masking under-damaging bugs).
fn assert_hover_toggles_fixed_cb(cb_prop: &str) {
    let html = format!(
        "<html><head><style>\
         body {{ margin: 0 }}\
         #anc {{ margin: 50px; width: 300px; height: 300px; }}\
         #anc:hover {{ {cb_prop} }}\
         </style></head><body>\
         <div id=anc>\
         <div id=target style='position: fixed; top: 10px; left: 10px; width: 20px; height: 20px'></div>\
         </div></body></html>"
    );
    let mut harness = Harness::from_html(&html);
    assert_eq!(
        harness.layout_rect("#target").x,
        10.0,
        "initial (viewport CB)"
    );

    // Hover #anc: it now establishes the fixed containing block
    harness.base_mut().set_hover_to(60.0, 60.0);
    harness.pump();
    assert_eq!(
        harness.layout_rect("#target").x,
        60.0,
        "hovered: `{cb_prop}` makes #anc the CB"
    );

    // Unhover: back to the viewport
    harness.base_mut().set_hover_to(700.0, 500.0);
    harness.pump();
    assert_eq!(
        harness.layout_rect("#target").x,
        10.0,
        "unhovered (viewport CB)"
    );
}

#[test]
fn hover_transform_toggles_fixed_containing_block() {
    assert_hover_toggles_fixed_cb("transform: translateX(0px)");
}

#[test]
fn hover_will_change_toggles_fixed_containing_block() {
    assert_hover_toggles_fixed_cb("will-change: transform");
}

#[test]
fn hover_filter_toggles_fixed_containing_block() {
    assert_hover_toggles_fixed_cb("filter: grayscale(50%)");
}

/// Dynamically inserting and removing a hoisted box (the common fixed-position
/// popup/modal pattern). The box must be laid out via its containing block on
/// insertion and fully disappear from layout/paint/hit-test on removal.
#[test]
fn insert_and_remove_hoisted_box() {
    let html = "<html><head><style>body { margin: 0 }</style></head><body>\
         <div id=anc style='margin: 50px; width: 300px; height: 300px'></div>\
         </body></html>";
    let mut harness = Harness::from_html(html);
    let anc = harness.node("#anc");

    // Insert a fixed box inside #anc
    let target = {
        let mut doc = harness.base_mut();
        let mut mutator = doc.mutate();
        let target = mutator.create_element(
            QualName::new(None, ns!(html), local_name!("div")),
            vec![blitz_dom::Attribute {
                name: style_attr(),
                value: "position: fixed; top: 10px; left: 10px; width: 20px; height: 20px".into(),
            }],
        );
        mutator.append_children(anc, &[target]);
        target
    };
    harness.pump();

    // Positioned against the viewport, and hit-testable there
    assert_eq!(harness.layout_rect_of(target).x, 10.0);
    assert_eq!(harness.layout_rect_of(target).y, 10.0);
    assert_eq!(harness.hit_node(15.0, 15.0), target);

    // Remove it again: it must stop being laid out / hit-testable
    harness.base_mut().mutate().remove_node(target);
    harness.pump();
    assert_ne!(harness.hit(15.0, 15.0).map(|h| h.node_id), Some(target));
}

/// Toggling `display: none` on a hoisted box's static parent must hide and
/// re-show the hoisted box.
#[test]
fn display_none_toggle_on_static_parent() {
    let html = "<html><head><style>body { margin: 0 }</style></head><body>\
         <div id=parent style='width: 300px; height: 300px'>\
         <div id=target style='position: fixed; top: 10px; left: 10px; width: 20px; height: 20px'></div>\
         </div></body></html>";
    let mut harness = Harness::from_html(html);
    let parent = harness.node("#parent");
    let target = harness.node("#target");

    assert_eq!(harness.hit_node(15.0, 15.0), target);

    set_style(
        &mut harness,
        parent,
        "display: none; width: 300px; height: 300px",
    );
    assert_ne!(
        harness.hit(15.0, 15.0).map(|h| h.node_id),
        Some(target),
        "hidden with its parent"
    );

    set_style(&mut harness, parent, "width: 300px; height: 300px");
    assert_eq!(
        harness.hit_node(15.0, 15.0),
        target,
        "re-shown with its parent"
    );
    assert_eq!(harness.layout_rect("#target").x, 10.0);
}

/// A layout change (not a CB change) inside a hoisted subtree must relayout the
/// hoisted box through its containing block.
#[test]
fn content_change_inside_hoisted_subtree() {
    let html = "<html><head><style>body { margin: 0 }</style></head><body>\
         <div style='width: 300px; height: 300px'>\
         <div id=target style='position: fixed; top: 10px; left: 10px'>\
         <div id=inner style='width: 20px; height: 20px'></div>\
         </div></div></body></html>";
    let mut harness = Harness::from_html(html);
    let inner = harness.node("#inner");

    assert_eq!(harness.layout_rect("#target").width, 20.0);

    set_style(&mut harness, inner, "width: 50px; height: 40px");
    assert_eq!(harness.layout_rect("#target").width, 50.0);
    assert_eq!(harness.layout_rect("#target").height, 40.0);
}
