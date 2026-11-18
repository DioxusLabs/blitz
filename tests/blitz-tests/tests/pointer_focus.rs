//! Pressing the pointer on an element focuses its nearest focusable inclusive
//! ancestor (including `tabindex="-1"` elements), and clears focus when there is
//! none. Mice move focus on pointerdown, fingers on a tap.

use blitz_test_harness::{Harness, HarnessOptions};

fn harness() -> Harness {
    Harness::from_html_with(
        r##"<html><body style="margin:0">
            <input id="text" type="text" style="display:block; width:100px; height:20px;">
            <button id="button" style="display:block; width:100px; height:20px;"><span id="label">Go</span></button>
            <div id="roving" tabindex="-1" style="width:100px; height:20px;"><span id="item">Item</span></div>
            <div id="plain" style="width:100px; height:20px;">Plain</div>
            <a id="link" href="#plain" style="display:block; width:100px; height:20px;">Link</a>
            <label id="for-text" for="text" style="display:block; width:100px; height:20px;">Text</label>
            <button id="disabled" disabled style="display:block; width:100px; height:20px;">Off</button>
        </body></html>"##,
        HarnessOptions {
            width: 200,
            height: 200,
            ..Default::default()
        },
    )
}

#[test]
fn clicking_a_button_focuses_it() {
    let mut harness = harness();
    harness.click("#text");
    harness.click("#label");
    assert_eq!(harness.focused(), Some(harness.node("#button")));
}

#[test]
fn clicking_inside_a_negative_tabindex_element_focuses_it() {
    let mut harness = harness();
    harness.click("#item");
    assert_eq!(harness.focused(), Some(harness.node("#roving")));
}

#[test]
fn clicking_a_link_focuses_it() {
    let mut harness = harness();
    harness.click("#link");
    assert_eq!(harness.focused(), Some(harness.node("#link")));
}

#[test]
fn focus_moves_on_pointerdown() {
    let mut harness = harness();
    let (x, y) = harness.center_of("#button");
    harness.mouse_down_at(x, y);
    assert_eq!(harness.focused(), Some(harness.node("#button")));
}

#[test]
fn clicking_unfocusable_content_clears_focus() {
    let mut harness = harness();
    harness.click("#button");
    harness.click("#plain");
    // With nothing focused, focus falls back to the root element
    assert_eq!(harness.focused(), Some(harness.node("html")));
}

#[test]
fn clicking_a_disabled_element_leaves_focus_unchanged() {
    let mut harness = harness();
    harness.click("#button");
    harness.click("#disabled");
    assert_eq!(harness.focused(), Some(harness.node("#button")));
}

#[test]
fn fingers_move_focus_on_tap_not_touchstart() {
    let mut harness = harness();
    harness.click("#text");
    let (x, y) = harness.center_of("#button");
    harness.touch_down(1, x, y);
    assert_eq!(harness.focused(), Some(harness.node("#text")));
    harness.touch_up(1, x, y);
    assert_eq!(harness.focused(), Some(harness.node("#button")));
}

#[test]
fn clicking_a_label_focuses_its_control() {
    let mut harness = harness();
    harness.click("#button");
    harness.click("#for-text");
    assert_eq!(harness.focused(), Some(harness.node("#text")));
}
