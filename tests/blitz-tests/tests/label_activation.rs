//! Clicking a label activates its control and focuses it, as browsers do.
//!
//! A click on a text input has no default action of its own (pointerdown is
//! what focuses it), so clicking the label of a text input or a textarea left
//! the focus where it was.

use blitz_dom::NodeId;
use blitz_test_harness::{Harness, HarnessOptions};

fn harness(html: &str) -> Harness {
    Harness::from_html_with(
        html,
        HarnessOptions {
            width: 300,
            height: 200,
            ..Default::default()
        },
    )
}

fn focused(harness: &Harness) -> Option<NodeId> {
    harness.base().get_focussed_node_id()
}

#[test]
fn clicking_a_label_focuses_its_text_input() {
    let mut harness = harness(
        r#"<html><body style="margin:0">
            <label id="label" for="input" style="display:block; height:20px;">Name</label>
            <input id="input">
        </body></html>"#,
    );

    harness.click("#label");

    assert_eq!(focused(&harness), Some(harness.node("#input")));
}

#[test]
fn clicking_a_label_focuses_its_textarea() {
    let mut harness = harness(
        r#"<html><body style="margin:0">
            <label id="label" for="notes" style="display:block; height:20px;">Notes</label>
            <textarea id="notes"></textarea>
        </body></html>"#,
    );

    harness.click("#label");

    assert_eq!(focused(&harness), Some(harness.node("#notes")));
}

#[test]
fn clicking_a_label_focuses_the_textarea_inside_it() {
    let mut harness = harness(
        r#"<html><body style="margin:0">
            <label style="display:block;">
                <span id="caption" style="display:block; height:20px;">Notes</span>
                <textarea id="notes"></textarea>
            </label>
        </body></html>"#,
    );

    harness.click("#caption");

    assert_eq!(focused(&harness), Some(harness.node("#notes")));
}

/// `disabled="true"` rather than the bare attribute: blitz parses `disabled` as
/// a boolean when computing focusability (see #845).
#[test]
fn clicking_a_label_leaves_a_disabled_control_unfocused() {
    let mut harness = harness(
        r#"<html><body style="margin:0">
            <label id="label" for="input" style="display:block; height:20px;">Name</label>
            <input id="input" disabled="true">
        </body></html>"#,
    );

    harness.click("#label");

    assert_ne!(focused(&harness), Some(harness.node("#input")));
}
