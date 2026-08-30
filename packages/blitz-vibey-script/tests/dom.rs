//! Tests for the JavaScript DOM APIs exposed by blitz-vibey-script

use blitz_dom::{Document, DocumentConfig};
use blitz_traits::events::DomEvent;
use blitz_vibey_script::ScriptDocument;
use keyboard_types::Modifiers;

fn doc_from_html(html: &str) -> ScriptDocument {
    let mut doc = ScriptDocument::from_html(html, DocumentConfig::default());
    doc.execute_scripts();
    doc
}

fn dispatch_click(doc: &mut ScriptDocument, selector: &str) {
    let click_event = {
        let inner = doc.inner();
        let node_id = inner.query_selector(selector).unwrap().unwrap();
        DomEvent::new(
            node_id,
            inner
                .get_node(node_id)
                .unwrap()
                .synthetic_click_event(Modifiers::empty()),
        )
    };
    doc.dispatch_dom_event(click_event);
}

fn text_of_selector(doc: &ScriptDocument, selector: &str) -> String {
    let inner = doc.inner();
    let node_id = inner
        .query_selector(selector)
        .unwrap()
        .unwrap_or_else(|| panic!("no node matching {selector}"));
    inner.get_node(node_id).unwrap().text_content()
}

#[test]
fn executes_inline_scripts() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"></div>
            <script>
                const el = document.createElement("h1");
                el.textContent = "Hello from JS";
                document.getElementById("root").appendChild(el);
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#root > h1"), "Hello from JS");
}

#[test]
fn scripts_run_in_document_order_and_share_globals() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"></div>
            <script>globalThis.counter = 1;</script>
            <script>globalThis.counter += 1;</script>
            <script>
                document.getElementById("root").textContent = `counter = ${counter}`;
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#root"), "counter = 2");
}

#[test]
fn dom_tree_manipulation() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <ul id="list"><li id="a">a</li><li id="c">c</li></ul>
            <script>
                const list = document.getElementById("list");
                const b = document.createElement("li");
                b.textContent = "b";
                list.insertBefore(b, document.getElementById("c"));

                // Move "a" to the end, then remove it
                const a = document.getElementById("a");
                list.appendChild(a);
                list.removeChild(a);

                const summary = document.createElement("div");
                summary.id = "summary";
                summary.textContent = [...list.childNodes].map((li) => li.textContent).join(",");
                document.body.appendChild(summary);
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#summary"), "b,c");
    assert_eq!(text_of_selector(&doc, "#list"), "bc");
}

#[test]
fn attributes_and_properties() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <div id="box" class="before" data-x="1"></div>
            <script>
                const box = document.getElementById("box");
                const results = [];
                results.push(box.getAttribute("class"));
                box.className = "after";
                results.push(box.getAttribute("class"));
                results.push(box.hasAttribute("data-x"));
                box.removeAttribute("data-x");
                results.push(box.hasAttribute("data-x"));
                box.setAttribute("title", "hello");
                results.push(box.getAttribute("title"));

                const out = document.createElement("div");
                out.id = "out";
                out.textContent = results.join("|");
                document.body.appendChild(out);
            </script>
        </body></html>
        "#,
    );
    assert_eq!(
        text_of_selector(&doc, "#out"),
        "before|after|true|false|hello"
    );
}

#[test]
fn query_selectors() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <div class="item">one</div>
            <div class="item special">two</div>
            <section><div class="item">three</div></section>
            <script>
                const out = document.createElement("div");
                out.id = "out";
                const all = document.querySelectorAll(".item").length;
                const special = document.querySelector(".item.special").textContent;
                const scoped = document.querySelector("section").querySelectorAll(".item").length;
                out.textContent = `${all}|${special}|${scoped}`;
                document.body.appendChild(out);
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#out"), "3|two|1");
}

#[test]
fn inner_html() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><span>old</span></div>
            <script>
                const root = document.getElementById("root");
                root.innerHTML = "<p class='msg'>new <b>content</b></p>";
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#root .msg"), "new content");
    let inner = doc.inner();
    assert!(inner.query_selector("#root span").unwrap().is_none());
}

#[test]
fn click_event_listeners() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <button id="btn">Click me</button>
            <div id="out">unclicked</div>
            <script>
                let clicks = 0;
                const btn = document.getElementById("btn");
                btn.addEventListener("click", (event) => {
                    clicks += 1;
                    const out = document.getElementById("out");
                    out.textContent = `clicked ${clicks} times; target=${event.target.tagName}; ct=${event.currentTarget.id}`;
                });
            </script>
        </body></html>
        "#,
    );

    let click_event = {
        let inner = doc.inner();
        let btn_id = inner.query_selector("#btn").unwrap().unwrap();
        DomEvent::new(
            btn_id,
            inner
                .get_node(btn_id)
                .unwrap()
                .synthetic_click_event(Modifiers::empty()),
        )
    };
    doc.dispatch_dom_event(click_event.clone());
    assert_eq!(
        text_of_selector(&doc, "#out"),
        "clicked 1 times; target=BUTTON; ct=btn"
    );
    doc.dispatch_dom_event(click_event);
    assert_eq!(
        text_of_selector(&doc, "#out"),
        "clicked 2 times; target=BUTTON; ct=btn"
    );
}

#[test]
fn click_events_bubble_and_stop_propagation() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="outer"><div id="middle"><button id="inner">hi</button></div></div>
            <div id="out"></div>
            <script>
                const log = [];
                const record = (name) => () => {
                    log.push(name);
                    document.getElementById("out").textContent = log.join(",");
                };
                document.getElementById("outer").addEventListener("click", record("outer"));
                document.getElementById("middle").addEventListener("click", (event) => {
                    record("middle")();
                    event.stopPropagation();
                });
                document.getElementById("inner").addEventListener("click", record("inner"));
            </script>
        </body></html>
        "#,
    );

    let click_event = {
        let inner = doc.inner();
        let btn_id = inner.query_selector("#inner").unwrap().unwrap();
        DomEvent::new(
            btn_id,
            inner
                .get_node(btn_id)
                .unwrap()
                .synthetic_click_event(Modifiers::empty()),
        )
    };
    doc.dispatch_dom_event(click_event);

    // "outer" should not be reached because "middle" stops propagation
    assert_eq!(text_of_selector(&doc, "#out"), "inner,middle");
}

#[test]
fn microtasks_run_after_script_execution() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <div id="out">pending</div>
            <script>
                Promise.resolve()
                    .then(() => "microtask")
                    .then((value) => {
                        document.getElementById("out").textContent = value;
                    });
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#out"), "microtask");
}

#[test]
fn timers_run_on_poll() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="out">pending</div>
            <script>
                setTimeout((suffix) => {
                    document.getElementById("out").textContent = "timer ran " + suffix;
                }, 5, "with args");
            </script>
        </body></html>
        "#,
    );

    assert_eq!(text_of_selector(&doc, "#out"), "pending");
    std::thread::sleep(std::time::Duration::from_millis(20));
    let ran = doc.poll(None);
    assert!(ran);
    assert_eq!(text_of_selector(&doc, "#out"), "timer ran with args");
}

#[test]
fn request_animation_frame_runs_on_poll() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="out">pending</div>
            <script>
                requestAnimationFrame(() => {
                    document.getElementById("out").textContent = "frame";
                });
            </script>
        </body></html>
        "#,
    );

    std::thread::sleep(std::time::Duration::from_millis(30));
    doc.poll(None);
    assert_eq!(text_of_selector(&doc, "#out"), "frame");
}

#[test]
fn input_value_property() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <input id="field" value="initial">
            <div id="out"></div>
            <script>
                const field = document.getElementById("field");
                const before = field.value;
                field.value = "updated";
                document.getElementById("out").textContent = `${before}|${field.value}`;
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#out"), "initial|updated");
}

#[test]
fn checkbox_click_fires_input_and_change_events() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <input type="checkbox" id="check">
            <div id="out"></div>
            <script>
                const check = document.getElementById("check");
                const log = [];
                check.addEventListener("input", () => log.push(`input:${check.checked}`));
                check.addEventListener("change", () => {
                    log.push(`change:${check.checked}`);
                    document.getElementById("out").textContent = log.join(",");
                });
            </script>
        </body></html>
        "#,
    );

    // Resolve style/layout: this constructs the checkbox's internal state
    // (as would happen before rendering in a windowed application)
    doc.inner_mut().resolve(0.0);

    let click_event = {
        let inner = doc.inner();
        let check_id = inner.query_selector("#check").unwrap().unwrap();
        DomEvent::new(
            check_id,
            inner
                .get_node(check_id)
                .unwrap()
                .synthetic_click_event(Modifiers::empty()),
        )
    };
    doc.dispatch_dom_event(click_event);
    assert_eq!(text_of_selector(&doc, "#out"), "input:true,change:true");
}

#[test]
fn dom_content_loaded_and_window_load_fire() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <div id="out"></div>
            <script>
                const log = [];
                document.addEventListener("DOMContentLoaded", () => log.push("dcl"));
                window.addEventListener("load", () => {
                    log.push("load");
                    document.getElementById("out").textContent = log.join(",");
                });
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#out"), "dcl,load");
}

#[test]
fn on_event_idl_properties_are_dispatched() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <button id="btn">go</button>
            <div id="out"></div>
            <script>
                document.getElementById("btn").onclick = (event) => {
                    document.getElementById("out").textContent = `onclick:${event.type}`;
                };
            </script>
        </body></html>
        "#,
    );

    let click_event = {
        let inner = doc.inner();
        let btn_id = inner.query_selector("#btn").unwrap().unwrap();
        DomEvent::new(
            btn_id,
            inner
                .get_node(btn_id)
                .unwrap()
                .synthetic_click_event(Modifiers::empty()),
        )
    };
    doc.dispatch_dom_event(click_event);
    assert_eq!(text_of_selector(&doc, "#out"), "onclick:click");
}

#[test]
fn node_wrappers_have_stable_identity() {
    let doc = doc_from_html(
        r##"
        <html><body>
            <div id="root"><span id="child">x</span></div>
            <div id="out"></div>
            <script>
                const root1 = document.getElementById("root");
                const root2 = document.querySelector("#root");
                root1.expando = "kept";
                const sameObject = root1 === root2;
                const viaParent = document.getElementById("child").parentNode;
                document.getElementById("out").textContent =
                    `${sameObject}|${viaParent === root1}|${viaParent.expando}`;
            </script>
        </body></html>
        "##,
    );
    assert_eq!(text_of_selector(&doc, "#out"), "true|true|kept");
}

#[test]
fn style_bindings() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <div id="box" style="color: red;"></div>
            <div id="out"></div>
            <script>
                const box = document.getElementById("box");
                const before = box.style.cssText;
                box.style.setProperty("background-color", "blue");
                const bg = box.style.getPropertyValue("background-color");
                document.getElementById("out").textContent = `${before}|${bg}`;
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#out"), "color: red;|blue");
}

// Dispatched events expose `getModifierState`, callable from listeners
// (React's synthetic events call it on the native event).
#[test]
fn event_get_modifier_state() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <button id="btn">go</button>
            <div id="out">unset</div>
            <script>
                document.getElementById("btn").addEventListener("click", (e) => {
                    const t = typeof e.getModifierState;
                    const shift = e.getModifierState("Shift");
                    const caps = e.getModifierState("CapsLock");
                    document.getElementById("out").textContent = `${t}:${shift}:${caps}`;
                });
            </script>
        </body></html>
        "#,
    );
    let click_event = {
        let inner = doc.inner();
        let btn_id = inner.query_selector("#btn").unwrap().unwrap();
        DomEvent::new(
            btn_id,
            inner
                .get_node(btn_id)
                .unwrap()
                .synthetic_click_event(Modifiers::SHIFT),
        )
    };
    doc.dispatch_dom_event(click_event);
    assert_eq!(text_of_selector(&doc, "#out"), "function:true:false");
}

// `hidden` reflects the boolean attribute (and the UA stylesheet hides it).
#[test]
fn element_hidden_reflection() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <div id="box"></div>
            <div id="out"></div>
            <script>
                const box = document.getElementById("box");
                const before = box.hidden;
                box.hidden = true;
                const attr = box.hasAttribute("hidden");
                box.hidden = false;
                const after = box.hasAttribute("hidden");
                document.getElementById("out").textContent = `${before}|${attr}|${after}`;
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#out"), "false|true|false");
}

// `selectionStart`/`selectionEnd` read and write the text input's real editor
// selection (in UTF-16 code units); `setSelectionRange` sets both. Non-input
// elements report null. React snapshots/restores the caret around controlled
// input re-renders using these.
//
// Note: needs a real viewport, since cursor placement requires a non-degenerate
// text layout.
#[test]
fn input_selection_offsets() {
    let mut doc = ScriptDocument::from_html(
        r#"
        <html><body>
            <input id="field" value="héllo">
            <div id="out"></div>
            <script>
                const f = document.getElementById("field");
                const inInput = ("selectionStart" in f);
                f.setSelectionRange(1, 3);
                const a = `${f.selectionStart}-${f.selectionEnd}`;
                f.selectionEnd = 5;
                f.selectionStart = 2;
                const b = `${f.selectionStart}-${f.selectionEnd}`;
                const divSel = document.getElementById("out").selectionStart;
                document.getElementById("out").textContent = `${inInput}|${a}|${b}|${divSel}`;
            </script>
        </body></html>
        "#,
        DocumentConfig {
            viewport: Some(blitz_traits::shell::Viewport::new(
                800,
                600,
                1.0,
                blitz_traits::shell::ColorScheme::Light,
            )),
            ..Default::default()
        },
    );
    doc.execute_scripts();
    assert_eq!(text_of_selector(&doc, "#out"), "true|1-3|2-5|null");
}

// Interface constructors referenced by `instanceof` probes exist as globals.
#[test]
fn interface_constructor_globals() {
    let doc = doc_from_html(
        r#"
        <html><body>
            <div id="box"></div>
            <div id="out"></div>
            <script>
                const box = document.getElementById("box");
                const kinds = [HTMLInputElement, EventTarget, KeyboardEvent, MouseEvent]
                    .map((iface) => typeof iface)
                    .join(",");
                const probe = box instanceof HTMLInputElement;
                const isElement = box instanceof Element;
                document.getElementById("out").textContent = `${kinds}|${probe}|${isElement}`;
            </script>
        </body></html>
        "#,
    );
    assert_eq!(
        text_of_selector(&doc, "#out"),
        "function,function,function,function|false|true"
    );
}

// ── Three-phase dispatch over the DOM chain ───────────────────────────

// Outside dispatch, `eventPhase` is 0; inside, the phase constants match the
// DOM walk (capture 1 / target 2 / bubble 3) and capture-only listeners fire
// only during the capture phase.
#[test]
fn event_phase_values_across_dispatch_phases() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="outer"><div id="inner"><button id="leaf">hi</button></div></div>
            <div id="out"></div>
            <script>
                globalThis.__phaseLog = [];
                const log = globalThis.__phaseLog;
                const leaf = document.getElementById("leaf");
                const idle = new Event("idle");
                log.push(`idle:${idle.eventPhase}`);
                document.getElementById("outer").addEventListener("click", (e) => {
                    log.push(`outer:${e.eventPhase}`);
                }, true);
                document.getElementById("inner").addEventListener("click", (e) => {
                    log.push(`inner:${e.eventPhase}`);
                });
                leaf.addEventListener("click", (e) => {
                    log.push(`leaf:${e.eventPhase}:${e.target === leaf}`);
                });
            </script>
        </body></html>
        "#,
    );
    dispatch_click(&mut doc, "#leaf");
    doc.eval(r#"document.getElementById("out").textContent = globalThis.__phaseLog.join("|");"#);
    assert_eq!(
        text_of_selector(&doc, "#out"),
        "idle:0|outer:1|leaf:2:true|inner:3"
    );
}

// `stopPropagation()` called during the capture phase keeps the event from
// reaching the target and bubble phases; later capture listeners on the same
// receiver still run.
#[test]
fn capture_phase_propagation_stops() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="outer"><div id="inner"><button id="leaf">hi</button></div></div>
            <div id="out"></div>
            <script>
                globalThis.__log = [];
                const log = globalThis.__log;
                const record = (name) => () => log.push(name);
                document.getElementById("outer").addEventListener("click", (e) => {
                    record("outer-cap")();
                    e.stopPropagation();
                }, true);
                document.getElementById("outer").addEventListener("click", record("outer-cap-2"), true);
                document.getElementById("inner").addEventListener("click", record("inner-bub"));
                document.getElementById("leaf").addEventListener("click", record("leaf"));
            </script>
        </body></html>
        "#,
    );
    dispatch_click(&mut doc, "#leaf");
    doc.eval(r#"document.getElementById("out").textContent = globalThis.__log.join("|");"#);
    assert_eq!(text_of_selector(&doc, "#out"), "outer-cap|outer-cap-2");
}

// `stopImmediatePropagation()` silences the remaining listeners of the same
// receiver within the current phase.
#[test]
fn stop_immediate_propagation_within_receiver() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <button id="leaf">hi</button>
            <div id="out"></div>
            <script>
                globalThis.__log = [];
                const log = globalThis.__log;
                const leaf = document.getElementById("leaf");
                leaf.addEventListener("click", () => log.push("third"), true);
                leaf.addEventListener("click", (e) => {
                    log.push("first");
                    e.stopImmediatePropagation();
                });
                leaf.addEventListener("click", () => log.push("second"));
            </script>
        </body></html>
        "#,
    );
    dispatch_click(&mut doc, "#leaf");
    doc.eval(r#"document.getElementById("out").textContent = globalThis.__log.join("|");"#);
    assert_eq!(text_of_selector(&doc, "#out"), "third|first");
}

// After dispatch finishes, the transient fields are cleared: an event
// reference kept by a listener reads `currentTarget: null` and
// `eventPhase: 0` afterwards.
#[test]
fn transient_dispatch_state_reset_after_dispatch() {
    let mut doc = doc_from_html(
        r#"
        <html><body><button id="leaf">hi</button><div id="out"></div>
        <script>
            globalThis.saved = null;
            document.getElementById("leaf").addEventListener("click", (e) => {
                globalThis.saved = e;
            });
        </script></body></html>
        "#,
    );
    dispatch_click(&mut doc, "#leaf");
    doc.eval(
        r#"document.getElementById("out").textContent =
            `${globalThis.saved.currentTarget === null}|${globalThis.saved.eventPhase}`;"#,
    );
    assert_eq!(text_of_selector(&doc, "#out"), "true|0");
}

// `event.target` is wrapped lazily, through the same wrapper cache — the
// wrapper seen by a listener is the very object `getElementById` returns.
#[test]
fn lazy_target_shares_wrapper_identity() {
    let mut doc = doc_from_html(
        r#"
        <html><body><button id="leaf">hi</button><div id="out"></div>
        <script>
            globalThis.same = null;
            document.getElementById("leaf").addEventListener("click", (e) => {
                globalThis.same = e.target === document.getElementById("leaf");
            });
        </script></body></html>
        "#,
    );
    dispatch_click(&mut doc, "#leaf");
    doc.eval(r#"document.getElementById("out").textContent = String(globalThis.same);"#);
    assert_eq!(text_of_selector(&doc, "#out"), "true");
}
