//! Tests for the event class layers themselves: `new EventTarget()`,
//! `Event` construction/flags, `dispatchEvent` on a standalone target, and
//! the transient dispatch state. No DOM tree involvement — results are
//! reported through `__blitz_send_message` and drained via
//! [`ScriptDocument::take_messages`].

use blitz_dom::DocumentConfig;
use blitz_vibey_script::ScriptDocument;

/// Evaluate `js` (an expression list) and collect each `push(value)` as one
/// reported message, in order.
fn eval_report(doc: &mut ScriptDocument, js: &str) -> Vec<String> {
    doc.eval(&format!(
        r#"__blitz_send_message.__values = [];
        const push = (value) => __blitz_send_message(String(value));
        (function() {{ {js} }})();
        "#
    ));
    doc.take_messages()
}

#[test]
fn event_target_is_constructible_and_dispatch_round_trips() {
    let mut doc = ScriptDocument::from_html("<body></body>", DocumentConfig::default());
    doc.execute_scripts();

    let result = eval_report(
        &mut doc,
        r#"
        const et = new EventTarget();
        let phase = null;
        let thisIsEt = null;
        let targetIsEt = null;
        et.addEventListener("hola", (event) => {
            phase = event.eventPhase;
            thisIsEt = event.currentTarget === et;
            targetIsEt = event.target === et;
        });
        const returned = et.dispatchEvent(new Event("hola"));
        push(returned); push(phase); push(thisIsEt); push(targetIsEt);
        push(et instanceof EventTarget);
    "#,
    );
    assert_eq!(result, ["true", "2", "true", "true", "true"]);
}

#[test]
fn event_target_dispatch_event_returns_not_default_prevented() {
    let mut doc = ScriptDocument::from_html("<body></body>", DocumentConfig::default());
    doc.execute_scripts();

    let result = eval_report(
        &mut doc,
        r#"
        const et = new EventTarget();
        et.addEventListener("hola", (event) => event.preventDefault());
        const canceled = et.dispatchEvent(new Event("hola", { cancelable: true }));
        const uncanceled = et.dispatchEvent(new Event("hola"));
        push(canceled); push(uncanceled);
    "#,
    );
    assert_eq!(result, ["false", "true"]);
}

#[test]
fn event_target_stop_immediate_propagation() {
    let mut doc = ScriptDocument::from_html("<body></body>", DocumentConfig::default());
    doc.execute_scripts();

    let result = eval_report(
        &mut doc,
        r#"
        const et = new EventTarget();
        const log = [];
        et.addEventListener("hola", (event) => {
            log.push("first");
            event.stopImmediatePropagation();
        });
        et.addEventListener("hola", () => log.push("second"));
        et.dispatchEvent(new Event("hola"));
        push(log[0] ?? "(none)");
    "#,
    );
    assert_eq!(result, ["first"]);
}

#[test]
fn event_target_once_listeners_are_removed_after_dispatch() {
    let mut doc = ScriptDocument::from_html("<body></body>", DocumentConfig::default());
    doc.execute_scripts();

    let result = eval_report(
        &mut doc,
        r#"
        const et = new EventTarget();
        let calls = 0;
        et.addEventListener("hola", () => { calls += 1; }, { once: true });
        et.dispatchEvent(new Event("hola"));
        et.dispatchEvent(new Event("hola"));
        push(calls);
    "#,
    );
    assert_eq!(result, ["1"]);
}

#[test]
fn event_target_transient_state_resets_after_dispatch() {
    let mut doc = ScriptDocument::from_html("<body></body>", DocumentConfig::default());
    doc.execute_scripts();

    let result = eval_report(
        &mut doc,
        r#"
        const et = new EventTarget();
        let saved = null;
        et.addEventListener("hola", (event) => { saved = event; });
        et.dispatchEvent(new Event("hola"));
        push(saved.currentTarget === null); push(saved.eventPhase);
        push(saved.defaultPrevented);
    "#,
    );
    assert_eq!(result, ["true", "0", "false"]);
}

#[test]
fn event_flags_outside_dispatch() {
    let mut doc = ScriptDocument::from_html("<body></body>", DocumentConfig::default());
    doc.execute_scripts();

    let result = eval_report(
        &mut doc,
        r#"
        const idle = new Event("idle");
        const plain = new Event("x");
        plain.preventDefault();
        const cancelable = new Event("y", { cancelable: true });
        cancelable.preventDefault();
        push(idle.eventPhase); push(idle.currentTarget); push(idle.isTrusted);
        push(idle.composed); push(plain.defaultPrevented);
        push(cancelable.defaultPrevented);
    "#,
    );
    assert_eq!(result, ["0", "null", "true", "false", "false", "true"]);
}

#[test]
fn interface_layers_reject_construction_with_browser_message() {
    let mut doc = ScriptDocument::from_html("<body></body>", DocumentConfig::default());
    doc.execute_scripts();

    let result = eval_report(
        &mut doc,
        r#"
        const messages = [];
        try { new Node(); } catch (err) { messages.push(err.message); }
        try { new Text(); } catch (err) { messages.push(err.message); }
        for (const message of messages) push(message);
    "#,
    );
    assert_eq!(
        result,
        [
            "Failed to construct 'Node': Illegal constructor",
            "Failed to construct 'Text': Illegal constructor",
        ]
    );
}
