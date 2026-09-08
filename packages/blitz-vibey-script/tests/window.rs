//! Tests for the `Window` class: the global object built by the host hooks is
//! a live window on the `EventTarget` layer chain — `globalThis` is the
//! window, its listeners and `on<event>` handlers live in its own
//! `EventTargetLayer` block, and its members (timers, viewport accessors,
//! scroll, `getComputedStyle`, `document`, `CSS`) come from
//! `Window.prototype`. Results are reported through `__blitz_send_message`
//! and drained via [`ScriptDocument::take_messages`].

use blitz_dom::{Document, DocumentConfig};
use blitz_traits::events::DomEvent;
use blitz_vibey_script::ScriptDocument;
use keyboard_types::Modifiers;

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

fn doc_from_html(html: &str) -> ScriptDocument {
    let mut doc = ScriptDocument::from_html(html, DocumentConfig::default());
    doc.execute_scripts();
    doc
}

fn text_of_selector(doc: &ScriptDocument, selector: &str) -> Option<String> {
    let inner = doc.inner();
    inner
        .query_selector(selector)
        .unwrap()
        .map(|id| inner.get_node(id).unwrap().text_content())
}

// ── Identity: the global object is the window ────────────────────────

// The global object built by the host hooks is a live `Window`: `globalThis`,
// `self`, `parent` and `top` are the same object, its prototype chain runs
// through `EventTarget`, and global var declarations land on it.
#[test]
fn window_is_the_global_object_and_an_event_target() {
    let mut doc = ScriptDocument::from_html("<body></body>", DocumentConfig::default());
    doc.execute_scripts();
    // Top-level `var` binds on the global object.
    doc.eval("var declared = 7;");

    let result = eval_report(
        &mut doc,
        r#"
        push(typeof window); push(typeof Window);
        push(globalThis === window);
        push(self === window);
        push(parent === window && top === window);
        push(window instanceof EventTarget);
        push(window instanceof Window);
        push(window.declared === 7 && globalThis.declared === 7);
        push(window.opener === null);
    "#,
    );
    assert_eq!(
        result,
        [
            "object", "function", "true", "true", "true", "true", "true", "true", "true"
        ]
    );
}

// The window's own document: the `document` getter returns the same wrapper
// as the bare global `document`.
#[test]
fn window_document_matches_the_global_document() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        push(window.document === document);
        push(window.document.body === document.body);
    "#,
    );
    assert_eq!(result, ["true", "true"]);
}

// `new Window()` is an illegal constructor, like every interface layer that
// carries no buildable own data.
#[test]
fn new_window_is_rejected() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        try { new Window(); push("constructed"); }
        catch (error) { push(error instanceof TypeError); }
    "#,
    );
    assert_eq!(result, ["true"]);
}

// ── The window as an event target ────────────────────────────────────

// `window.dispatchEvent` dispatches through the window's own `EventTarget`
// layer, with the window as target and currentTarget at `AT_TARGET`.
#[test]
fn window_dispatch_event_runs_on_its_own_layer() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        let phase = null, targetOk = null, currentOk = null, thisOk = null;
        window.addEventListener("wobble", function (event) {
            phase = event.eventPhase;
            targetOk = event.target === window;
            currentOk = event.currentTarget === window;
            thisOk = this === window;
        });
        const returned = window.dispatchEvent(new Event("wobble"));
        push(returned); push(phase); push(targetOk); push(currentOk); push(thisOk);
    "#,
    );
    assert_eq!(result, ["true", "2", "true", "true", "true"]);
}

// Bare `addEventListener`/`removeEventListener` calls bind the window: they
// register on the window's own layer, removable again via
// `window.removeEventListener`.
#[test]
fn bare_listener_functions_bind_the_window() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        let hits = 0;
        const bump = () => { hits += 1; };
        addEventListener("thud", bump);
        window.dispatchEvent(new Event("thud"));
        removeEventListener("thud", bump);
        window.dispatchEvent(new Event("thud"));
        push(hits);
    "#,
    );
    assert_eq!(result, ["1"]);
}

// WebIDL semantics: an interface operation detached from its receiver is
// called with `this === undefined`, which binds the global this — the
// window — exactly like a bare call. So a detached `addEventListener`
// registers on the window, matching the browser (the callback then runs
// with `this === window`).
#[test]
fn detached_listener_binds_the_window() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        let hitTarget = null;
        const bump = function () { hitTarget = this; };
        const add = EventTarget.prototype.addEventListener;
        const remove = EventTarget.prototype.removeEventListener;
        add("click", bump);
        window.dispatchEvent(new Event("click"));
        push(hitTarget === window);
        remove("click", bump);
        window.dispatchEvent(new Event("click"));
        push(hitTarget === window);
    "#,
    );
    assert_eq!(result, ["true", "true"]);
}

// The same WebIDL rule covers `dispatchEvent`: a detached or bare call
// dispatches on the window (the global this), not on `undefined`.
#[test]
fn detached_dispatch_event_binds_the_window() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        let hits = 0;
        const bump = () => { hits += 1; };
        const dispatch = EventTarget.prototype.dispatchEvent;
        window.addEventListener("click", bump);
        dispatch(new Event("click"));
        push(hits);
    "#,
    );
    assert_eq!(result, ["1"]);
}

// A `once` window listener is removed right before its call: it fires on the
// first dispatch only, and a same-shaped re-registration after removal is
// accepted again.
#[test]
fn window_once_listener_removed_after_dispatch() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        let hits = 0;
        const bump = () => { hits += 1; };
        window.addEventListener("zap", bump, { once: true });
        window.dispatchEvent(new Event("zap"));
        window.dispatchEvent(new Event("zap"));
        window.addEventListener("zap", bump, { once: true });
        window.dispatchEvent(new Event("zap"));
        push(hits);
    "#,
    );
    assert_eq!(result, ["2"]);
}

// A capture-registered window listener fires on a window-targeted dispatch:
// the capture flavor runs before the non-capture one on the single target.
#[test]
fn window_capture_listener_fires_on_single_target() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        const order = [];
        window.addEventListener("sweep", () => order.push("capture"), { capture: true });
        window.addEventListener("sweep", () => order.push("bubble"));
        window.dispatchEvent(new Event("sweep"));
        push(order.join(","));
    "#,
    );
    assert_eq!(result, ["capture,bubble"]);
}

// A listener object's `handleEvent` receives the event with the object as
// `this`, like on any event target.
#[test]
fn window_handle_event_listener_object() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        let thisIsListener = null;
        const listener = { handleEvent(event) { thisIsListener = this === listener; } };
        window.addEventListener("ding", listener);
        window.dispatchEvent(new Event("ding"));
        push(thisIsListener);
    "#,
    );
    assert_eq!(result, ["true"]);
}

// `stopImmediatePropagation` inside a window listener halts the remaining
// window listeners, and `preventDefault` makes the dispatch return false.
#[test]
fn window_stop_immediate_and_prevent_default() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        const fired = [];
        window.addEventListener("snap", (event) => { fired.push("first"); event.stopImmediatePropagation(); });
        window.addEventListener("snap", () => fired.push("second"));
        const stopped = window.dispatchEvent(new Event("snap"));
        push(fired.join(","));
        push(stopped);

        window.addEventListener("snapped", (event) => event.preventDefault());
        push(window.dispatchEvent(new Event("snapped", { cancelable: true })));
    "#,
    );
    assert_eq!(result, ["first", "true", "false"]);
}

// ── Bubbling DOM events reach the window ─────────────────────────────

// An element event that bubbles reaches the window's registered listeners:
// phase is `BUBBLING_PHASE`, `target` is the element and `currentTarget` is
// the window.
#[test]
fn bubbling_click_reaches_window_listeners() {
    let mut doc = doc_from_html(r#"<body><button id="b"></button></body>"#);
    let result = eval_report(
        &mut doc,
        r#"
        let phase = null, targetOk = null, currentOk = null;
        window.addEventListener("click", (event) => {
            phase = event.eventPhase;
            targetOk = event.target === document.getElementById("b");
            currentOk = event.currentTarget === window;
        });
        document.getElementById("b").dispatchEvent(new MouseEvent("click", { bubbles: true }));
        push(phase); push(targetOk); push(currentOk);
    "#,
    );
    assert_eq!(result, ["3", "true", "true"]);
}

// `stopPropagation` in an element listener keeps the bubbling event from
// reaching the window's listeners.
#[test]
fn element_stop_propagation_keeps_window_listeners_silent() {
    let mut doc = doc_from_html(r#"<body><button id="b"></button></body>"#);
    let result = eval_report(
        &mut doc,
        r#"
        let hits = 0;
        document.getElementById("b").addEventListener("click", (event) => event.stopPropagation());
        window.addEventListener("click", () => { hits += 1; });
        document.getElementById("b").dispatchEvent(new MouseEvent("click", { bubbles: true }));
        push(hits);
    "#,
    );
    assert_eq!(result, ["0"]);
}

// ── `on<event>` handlers on the window ───────────────────────────────

// `on<event>` attributes on the window are attribute listeners in its own
// layer: assigning one fires when an element event bubbles out of the
// document, the getter reflects it, and assigning null removes it.
#[test]
fn window_onclick_attribute_fires_on_bubbling_click() {
    let mut doc = doc_from_html(r#"<body><button id="b"></button></body>"#);
    let result = eval_report(
        &mut doc,
        r#"
        push("onclick" in window);
        let hits = 0, currentIsWindow = null;
        window.onclick = (event) => { hits += 1; currentIsWindow = event.currentTarget === window; };
        push(typeof window.onclick);
        document.getElementById("b").dispatchEvent(new MouseEvent("click", { bubbles: true }));
        window.onclick = null;
        document.getElementById("b").dispatchEvent(new MouseEvent("click", { bubbles: true }));
        push(hits); push(currentIsWindow);
        push(window.onclick === null);
    "#,
    );
    assert_eq!(result, ["true", "function", "1", "true", "true"]);
}

// The window-reflecting handler types are present as IDL attributes.
#[test]
fn window_event_handler_types_are_present() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        push("onload" in window);
        push("onerror" in window);
        push("onhashchange" in window);
        push("onresize" in window);
        push("onunload" in window);
        push("onmouseover" in window);
        push("onkeydown" in window);
    "#,
    );
    assert_eq!(result, ["true"; 7]);
}

// `window.onload` assigned by a script fires when the runtime dispatches the
// window's `load` event after the document's scripts have run.
#[test]
fn window_onload_assignment_fires_on_load() {
    let mut doc = ScriptDocument::from_html(
        r#"<html><body><script>window.onload = () => __blitz_send_message("loaded");</script></body></html>"#,
        DocumentConfig::default(),
    );
    doc.execute_scripts();
    assert_eq!(doc.take_messages(), ["loaded"]);
}

// A `<body onload="...">` attribute is installed as the window's `load`
// handler (the window-reflecting body element event handler set), firing
// with the window's `load` event.
#[test]
fn body_onload_attribute_installs_window_handler() {
    let mut doc = ScriptDocument::from_html(
        r#"<html><body onload="__blitz_send_message('body-onload')"></body></html>"#,
        DocumentConfig::default(),
    );
    doc.execute_scripts();
    assert_eq!(doc.take_messages(), ["body-onload"]);
}

// `on<event>` assignments through the window-reflecting `<body>` handlers
// land on the window's attribute listeners, in both directions.
#[test]
fn body_window_reflecting_handlers_forward_to_window() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        const boom = () => {};
        window.onerror = boom;
        push(document.body.onerror === boom);
        document.body.onhashchange = boom;
        push(window.onhashchange === boom);
        document.body.onerror = null;
        push(window.onerror === null);
    "#,
    );
    assert_eq!(result, ["true", "true", "true"]);
}

// ── Window members ───────────────────────────────────────────────────

// `location` and `navigator` are data properties of `Window.prototype`
// (the spec's unforgeable `location` and `SameObject` `navigator`): reads
// and identity checks go through the prototype, and assignment lands on the
// prototype's property.
#[test]
fn window_location_and_navigator_are_prototype_members() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        push(typeof window.location);
        push("location" in window);
        push("navigator" in window);
        push(window.location.href);
        push(window.navigator.userAgent);
        push(window.location === window.location);
        push(window.navigator === window.navigator);
        // The getters have no setters: assignment silently fails in
        // non-strict code, and the [SameObject] values survive.
        window.location = 42;
        window.navigator = 42;
        push(window.location.href);
        push(window.navigator === 42);
    "#,
    );
    assert_eq!(
        result,
        [
            "object",
            "true",
            "true",
            "about:blank",
            "Mozilla/5.0 (compatible; Blitz)",
            "true",
            "true",
            "about:blank",
            "false"
        ]
    );
}

// Timer, viewport, scrolling, style and namespace members live on
// `Window.prototype` (so bare calls resolve through the global object).
#[test]
fn window_members_live_on_the_prototype() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        push(typeof window.setTimeout);
        push(typeof window.setInterval);
        push(typeof window.requestAnimationFrame);
        push(typeof window.clearTimeout);
        push(typeof window.getComputedStyle);
        push(typeof window.innerWidth);
        push(typeof window.scrollX);
        push(typeof window.scroll);
        push(typeof window.CSS);
        push(typeof CSS.escape);
        push(CSS.supports("color: red"));
    "#,
    );
    assert_eq!(
        result,
        [
            "function", "function", "function", "function", "function", "number", "number",
            "function", "object", "function", "true"
        ]
    );
}

// Timers registered through the window return an id that
// `clearTimeout`/`cancelAnimationFrame` accept.
#[test]
fn window_timer_members_register_and_cancel() {
    let mut doc = doc_from_html("<body></body>");
    let result = eval_report(
        &mut doc,
        r#"
        const id = window.setTimeout(() => {}, 1000);
        window.clearTimeout(id);
        const raf = window.requestAnimationFrame(() => {});
        window.cancelAnimationFrame(raf);
        push(id > 0 && raf > 0);
    "#,
    );
    assert_eq!(result, ["true"]);
}

// Viewport dimension and scroll getters return numbers, scrolling methods
// run without error, and `getComputedStyle` returns an object.
#[test]
fn window_viewport_scroll_and_style_members() {
    let mut doc = doc_from_html("<body><div id='t' style='color: red'></div></body>");
    let result = eval_report(
        &mut doc,
        r#"
        push(typeof window.innerWidth);
        push(typeof window.innerHeight);
        push(typeof window.outerWidth);
        push(typeof window.devicePixelRatio);
        push(typeof window.scrollY);
        push(typeof window.pageXOffset);
        window.scroll(0, 10);
        window.scrollBy(0, 5);
        const style = window.getComputedStyle(document.getElementById("t"));
        push(typeof style === "object" && style.color === "rgb(255, 0, 0)");
    "#,
    );
    assert_eq!(
        result,
        [
            "number", "number", "number", "number", "number", "number", "true"
        ]
    );
}

// The window's listeners live in its own `EventTarget` layer, which is kept
// alive by the global object: they survive a forced GC and keep firing.
#[test]
fn window_listeners_survive_gc() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <button id="b"></button>
            <div id="out"></div>
            <script>
                window.addEventListener("click", () => {
                    document.getElementById("out").textContent = "hit";
                });
            </script>
        </body></html>
        "#,
    );
    doc.run_gc();

    let click_event = {
        let inner = doc.inner();
        let node_id = inner.query_selector("#b").unwrap().unwrap();
        DomEvent::new(
            node_id,
            inner
                .get_node(node_id)
                .unwrap()
                .synthetic_click_event(Modifiers::empty()),
        )
    };
    doc.dispatch_dom_event(click_event);

    assert_eq!(text_of_selector(&doc, "#out"), Some("hit".to_string()));
}
