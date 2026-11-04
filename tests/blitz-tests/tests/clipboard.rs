use blitz_test_harness::Harness;
use blitz_traits::events::UiEvent;

#[test]
fn paste_dispatches_input_only_with_focused_text_input() {
    let mut harness = Harness::from_html("<html><body><input autofocus></body></html>");
    let input = harness.focused().unwrap();
    let events = harness.dispatch_recorded([UiEvent::Paste("from clipboard".into())]);
    assert_eq!(events, ["input"]);
    assert_eq!(
        harness
            .base()
            .get_node(input)
            .unwrap()
            .element_data()
            .unwrap()
            .text_input_data()
            .unwrap()
            .editor
            .text(),
        "from clipboard"
    );
    let mut harness = Harness::from_html("<html><body><input></body></html>");
    assert!(
        harness
            .dispatch_recorded([UiEvent::Paste("ignored".into())])
            .is_empty()
    );
    let mut harness =
        Harness::from_html("<html><body><button autofocus>Button</button></body></html>");
    assert_eq!(harness.focused(), Some(harness.node("button")));
    assert!(
        harness
            .dispatch_recorded([UiEvent::Paste("ignored".into())])
            .is_empty()
    );
}
