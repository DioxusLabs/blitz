//! A falsy Dioxus boolean attribute must not reach the DOM, because HTML
//! treats the mere presence of `disabled` (or `checked`) as true.

use blitz_test_harness::Harness;
use dioxus::prelude::*;

fn toggle_disabled_app() -> Element {
    let mut disabled = use_signal(|| false);
    rsx! {
        button {
            id: "toggle",
            style: "width: 100px; height: 30px; display: block;",
            onclick: move |_| disabled.toggle(),
            "Toggle"
        }
        button {
            id: "target",
            style: "width: 100px; height: 30px; display: block;",
            disabled: disabled(),
            "Target"
        }
    }
}

#[test]
fn dioxus_falsy_disabled_is_removed() {
    let mut harness = Harness::from_component(toggle_disabled_app);

    // Initially `disabled: false`: no attribute, and the button is not :disabled
    assert_eq!(harness.attr("#target", "disabled"), None);
    assert_eq!(harness.query("#target:disabled"), None);

    harness.click("#toggle");
    assert!(harness.attr("#target", "disabled").is_some());
    assert!(harness.query("#target:disabled").is_some());

    // Toggling back to false removes the attribute again
    harness.click("#toggle");
    assert_eq!(harness.attr("#target", "disabled"), None);
    assert_eq!(harness.query("#target:disabled"), None);
}
