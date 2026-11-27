use blitz_dom::DocumentConfig;
use blitz_vibey_script::ScriptDocument;

fn doc_with_loop_limit(limit: u64) -> ScriptDocument {
    let mut doc = ScriptDocument::from_html("<p>limits</p>", DocumentConfig::default());
    doc.runtime_limits_mut().set_loop_iteration_limit(limit);
    doc
}

#[test]
fn loop_iteration_limit_stops_an_infinite_loop() {
    let mut doc = doc_with_loop_limit(10_000);
    doc.eval("while (true) {}");
    let errors = doc.take_js_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("RuntimeLimitError"), "{errors:?}");

    // The document is still usable afterwards.
    doc.eval("__blitz_send_message(String(1 + 1))");
    assert_eq!(doc.take_js_errors(), Vec::<String>::new());
    assert_eq!(doc.take_messages(), vec!["2".to_string()]);
}

#[test]
fn loop_iteration_limit_cannot_be_caught() {
    let mut doc = doc_with_loop_limit(10_000);
    doc.eval("var caught = false; try { while (true) {} } catch (e) { caught = true; }");
    assert_eq!(doc.take_js_errors().len(), 1);
    doc.eval("__blitz_send_message(String(caught))");
    assert_eq!(doc.take_messages(), vec!["false".to_string()]);
}

#[test]
fn loops_below_the_limit_run_normally() {
    let mut doc = doc_with_loop_limit(10_000);
    doc.eval(
        "var s = 0; for (var i = 0; i < 5000; i++) { s += i; } __blitz_send_message(String(s))",
    );
    assert_eq!(doc.take_js_errors(), Vec::<String>::new());
    assert_eq!(doc.take_messages(), vec!["12497500".to_string()]);
}
