//! `pointer-events: none` removes an element as a hit-test target: pointer
//! events pass through to whatever is underneath. Descendants are skipped via
//! inheritance, but a descendant that restores `pointer-events: auto` is
//! targetable again (css-ui-4).

use blitz_test_harness::{Harness, HarnessOptions};

fn harness(html: &str) -> Harness {
    Harness::from_html_with(
        html,
        HarnessOptions {
            width: 200,
            height: 200,
            ..Default::default()
        },
    )
}

fn is_checked(harness: &Harness, selector: &str) -> bool {
    let id = harness.node(selector);
    harness
        .base()
        .get_node(id)
        .and_then(|node| node.element_data())
        .and_then(|el| el.checkbox_input_checked())
        .unwrap()
}

#[test]
fn overlay_with_pointer_events_none_passes_through_to_button() {
    // The titlebar shape: an in-flow button row with a full-size positioned
    // overlay (paints and hit-tests above in-flow content) that sets
    // pointer-events: none.
    let harness = harness(
        r#"<html><body style="margin:0">
        <div style="position:relative; width:200px; height:36px;">
            <button id="btn" style="position:static; width:44px; height:36px; margin-left:156px; display:block;">x</button>
            <div id="overlay" style="position:absolute; left:0; top:0; right:0; bottom:0; pointer-events:none;">
                <span>Kopuz</span>
            </div>
        </div>
    </body></html>"#,
    );

    let btn = harness.node("#btn");
    let hit = harness.hit_node(178.0, 18.0);
    // The hit may be the button itself or its text child; resolve via ancestors
    let doc = harness.base();
    let hit_or_ancestor =
        std::iter::successors(Some(hit), |&id| doc.get_node(id).and_then(|n| n.parent))
            .any(|id| id == btn);
    assert!(
        hit_or_ancestor,
        "expected hit on #btn (node {btn}) but hit node {hit}"
    );
}

#[test]
fn element_with_pointer_events_none_is_not_a_target() {
    let harness = harness(
        r#"<html><body style="margin:0">
        <div id="under" style="width:100px; height:100px;">
            <div id="blocker" style="width:100px; height:100px; pointer-events:none;"></div>
        </div>
    </body></html>"#,
    );

    let blocker = harness.node("#blocker");
    let under = harness.node("#under");
    let hit = harness.hit_node(50.0, 50.0);
    assert_ne!(hit, blocker, "pointer-events:none element must not be hit");
    assert_eq!(hit, under, "the event should fall through to the parent");
}

#[test]
fn descendant_restoring_pointer_events_auto_is_targetable() {
    let harness = harness(
        r#"<html><body style="margin:0">
        <div style="pointer-events:none; width:200px; height:100px;">
            <div id="inner" style="pointer-events:auto; width:50px; height:50px;"></div>
        </div>
    </body></html>"#,
    );

    let inner = harness.node("#inner");
    assert_eq!(harness.hit_node(25.0, 25.0), inner);
}

#[test]
fn text_inside_pointer_events_none_overlay_is_not_a_target() {
    let harness = harness(
        r#"<html><body style="margin:0">
        <div id="under" style="position:relative; width:200px; height:36px;">
            <div id="overlay" style="position:absolute; left:0; top:0; right:0; bottom:0; pointer-events:none;">
                <span id="label" style="font-size:20px;">KOPUZKOPUZKOPUZ</span>
            </div>
        </div>
    </body></html>"#,
    );

    let overlay = harness.node("#overlay");
    let label = harness.node("#label");
    let hit = harness.hit_node(30.0, 14.0);
    assert_ne!(hit, overlay);
    assert_ne!(
        hit, label,
        "text in a pointer-events:none subtree must not be hit"
    );
}

#[test]
fn clicking_radio_without_name_does_not_panic() {
    let mut harness = harness(
        r#"<html><body style="margin:0">
            <input id="radio" type="radio" style="width:20px; height:20px;">
        </body></html>"#,
    );

    harness.click_at(10.0, 10.0);

    assert!(is_checked(&harness, "#radio"));
}
