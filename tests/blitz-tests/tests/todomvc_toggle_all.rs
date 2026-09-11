use blitz_test_harness::Harness;
use keyboard_types::Key;

#[path = "../../../examples/todomvc/src/app.rs"]
mod todomvc_app;

#[test]
fn todomvc_toggle_all_has_visible_label_content() {
    let mut harness = Harness::from_component(todomvc_app::app);

    harness.type_text("first todo");
    harness.press(Key::Enter);
    harness.type_text("second todo");
    harness.press(Key::Enter);

    assert!(
        harness.query(".toggle-all").is_some(),
        "toggle-all control should be rendered once at least one todo exists"
    );

    assert_eq!(harness.text_content(".toggle-all"), ">");

    let label_rect = harness.layout_rect(".toggle-all");
    assert!(label_rect.width > 0.0, "toggle-all control should have width");
    assert!(
        label_rect.height > 0.0,
        "toggle-all control should have height"
    );

    harness.click(".toggle-all");
    assert_eq!(
        harness.query_all(".todo-list li.completed").len(),
        2,
        "clicking toggle-all should mark all todos as completed"
    );

    harness.click(".toggle-all");
    assert_eq!(
        harness.query_all(".todo-list li.completed").len(),
        0,
        "clicking toggle-all again should unmark all todos"
    );
}
