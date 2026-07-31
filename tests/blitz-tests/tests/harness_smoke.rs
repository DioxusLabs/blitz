//! End-to-end smoke tests for the `blitz-testing` harness itself, covering
//! document construction, inspection, and input synthesis for both
//! `HtmlDocument` and `DioxusDocument` backed harnesses.

use blitz_test_harness::Harness;
use dioxus::prelude::*;

#[test]
fn html_inspection() {
    let harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <div id="box" class="a b" style="width:100px; height:50px; margin-left:20px; margin-top:10px;">Hello</div>
        </body></html>"#,
    );

    let rect = harness.layout_rect("#box");
    assert_eq!(rect.x, 20.0);
    assert_eq!(rect.y, 10.0);
    assert_eq!(rect.width, 100.0);
    assert_eq!(rect.height, 50.0);
    assert_eq!(harness.center_of("#box"), (70.0, 35.0));

    assert_eq!(harness.text_content("#box"), "Hello");
    assert_eq!(harness.attr("#box", "class").as_deref(), Some("a b"));
    assert_eq!(harness.query("#missing"), None);
    assert_eq!(harness.query_all("div").len(), 1);

    let dump = harness.dom_string();
    assert!(dump.contains("<div #box .a .b> @ (20,10) 100x50"), "{dump}");
    assert!(dump.contains("\"Hello\""), "{dump}");
}

#[test]
fn html_click_focuses_and_checks_checkbox() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="check" type="checkbox" style="width:20px; height:20px;">
        </body></html>"#,
    );

    let checkbox = harness.node("#check");
    harness.click("#check");

    assert_eq!(harness.focused(), Some(checkbox));
    let is_checked = harness
        .base()
        .get_node(checkbox)
        .and_then(|node| node.element_data())
        .and_then(|el| el.checkbox_input_checked())
        .unwrap();
    assert!(is_checked);
}

#[test]
fn html_type_text_into_input() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="text" type="text" style="width:200px; height:20px;">
        </body></html>"#,
    );

    harness.click("#text");
    harness.type_text("hi");

    let node_id = harness.node("#text");
    let doc = harness.base();
    let element = doc.get_node(node_id).unwrap().element_data().unwrap();
    let text_input = element.text_input_data().unwrap();
    assert_eq!(text_input.editor.text(), "hi");
}

#[test]
fn html_wheel_scrolls_overflow_container() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <div id="scroller" style="width:100px; height:100px; overflow-y:scroll;">
                <div style="width:50px; height:1000px;"></div>
            </div>
        </body></html>"#,
    );

    harness.wheel_at(50.0, 50.0, 0.0, -100.0);

    let node_id = harness.node("#scroller");
    let scroll_y = harness.base().get_node(node_id).unwrap().scroll_offset().y;
    assert!(scroll_y > 0.0, "expected scroll offset > 0, got {scroll_y}");
}

fn counter_app() -> Element {
    let mut count = use_signal(|| 0);
    rsx! {
        button {
            id: "increment",
            style: "width: 100px; height: 30px; display: block;",
            onclick: move |_| count += 1,
            "Increment"
        }
        div { id: "count", "Count: {count}" }
    }
}

#[test]
fn dioxus_click_updates_component_state() {
    let mut harness = Harness::from_component(counter_app);

    assert_eq!(harness.text_content("#count"), "Count: 0");

    harness.click("#increment");
    assert_eq!(harness.text_content("#count"), "Count: 1");

    harness.click("#increment");
    assert_eq!(harness.text_content("#count"), "Count: 2");
}
