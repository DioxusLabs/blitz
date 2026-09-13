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

// `document.styleSheets` / `element.sheet` expose stylesheets with a live,
// mutable `cssRules` list.
#[test]
fn cssom_stylesheet_rules() {
    let doc = doc_from_html(
        r##"
        <html><head>
            <style id="first">.a { color: red; } @media (min-width: 10px) { .b { margin: 1px } }</style>
        </head><body>
            <style id="second"></style>
            <div id="out"></div>
            <script>
                const sheets = document.styleSheets;
                const first = document.getElementById("first").sheet;
                const second = document.getElementById("second").sheet;
                const parts = [];
                parts.push(sheets.length, sheets[0] === first, sheets.item(1) === second);
                parts.push(first.ownerNode.id, first instanceof CSSStyleSheet, second.cssRules.length);

                const rule = first.cssRules[0];
                parts.push(rule.constructor.name, rule.type, rule.selectorText, rule.cssText);
                parts.push(rule.style.color, rule.style.getPropertyValue("color"), rule.parentStyleSheet === first);

                const media = first.cssRules[1];
                parts.push(media.constructor.name, media.conditionText, media.cssRules.length);
                parts.push(media.cssRules[0].parentRule === media, media.cssRules[0].style.margin);

                second.insertRule("p { padding: 2px }", 0);
                second.insertRule("h1 { padding: 3px }", 1);
                parts.push(second.cssRules.length, second.cssRules[1].selectorText);
                second.deleteRule(0);
                parts.push(second.cssRules.length, second.cssRules[0].selectorText);

                let err = "none";
                try { second.insertRule("not a rule", 0); } catch (e) { err = e.name; }
                parts.push(err);
                try { second.deleteRule(5); } catch (e) { err = e.name; }
                parts.push(err);

                document.getElementById("out").textContent = parts.join("|");
            </script>
        </body></html>
        "##,
    );
    assert_eq!(
        text_of_selector(&doc, "#out"),
        "2|true|true|first|true|0\
         |CSSStyleRule|1|.a|.a { color: red; }|red|red|true\
         |CSSMediaRule|(min-width: 10px)|1|true|1px\
         |2|h1|1|h1\
         |SyntaxError|IndexSizeError"
    );
}

// Rules inserted or modified through the CSSOM take effect on computed styles.
#[test]
fn cssom_mutations_restyle() {
    let doc = doc_from_html(
        r##"
        <html><head><style id="sheet">.box { width: 10px }</style></head><body>
            <div id="box" class="box"></div>
            <div id="out"></div>
            <script>
                const box = document.getElementById("box");
                const sheet = document.getElementById("sheet").sheet;
                const parts = [getComputedStyle(box).width];
                sheet.insertRule("#box { width: 20px }", 1);
                parts.push(getComputedStyle(box).width);
                sheet.cssRules[1].style.setProperty("width", "30px");
                parts.push(getComputedStyle(box).width, sheet.cssRules[1].cssText);
                sheet.cssRules[1].style.cssText = "width: 40px; height: 5px";
                parts.push(getComputedStyle(box).width, sheet.cssRules[1].style.length);
                sheet.cssRules[1].style.removeProperty("width");
                parts.push(getComputedStyle(box).width);
                sheet.deleteRule(0);
                parts.push(getComputedStyle(box).width);
                document.getElementById("out").textContent = parts.join("|");
            </script>
        </body></html>
        "##,
    );
    assert_eq!(
        text_of_selector(&doc, "#out"),
        "10px|20px|30px|#box { width: 30px; }|40px|2|10px|0px"
    );
}

// `@font-face` and `@keyframes` rules expose their descriptors / keyframes.
#[test]
fn cssom_font_face_and_keyframes() {
    let doc = doc_from_html(
        r##"
        <html><head>
            <style id="sheet">
                @font-face { font-family: "Foo"; src: url(foo.ttf) }
                @keyframes spin { from { opacity: 0 } to { opacity: 1 } }
            </style>
        </head><body>
            <div id="out"></div>
            <script>
                const sheet = document.getElementById("sheet").sheet;
                const parts = [];
                const face = sheet.cssRules[0];
                parts.push(face.constructor.name, face.type, face.style.getPropertyValue("font-family"));
                parts.push(face.style.getPropertyValue("src") !== "", face.style.fontFamily);
                const frames = sheet.cssRules[1];
                parts.push(frames.constructor.name, frames.name, frames.cssRules.length);
                parts.push(frames.cssRules[0].keyText, frames.cssRules[1].style.opacity);
                frames.appendRule("50% { opacity: 0.5 }");
                parts.push(frames.cssRules.length, frames.findRule("50%").cssText);
                frames.deleteRule("from");
                parts.push(frames.cssRules.length, frames.cssRules[0].keyText);
                document.getElementById("out").textContent = parts.join("|");
            </script>
        </body></html>
        "##,
    );
    assert_eq!(
        text_of_selector(&doc, "#out"),
        "CSSFontFaceRule|5|\"Foo\"|true|\"Foo\"\
         |CSSKeyframesRule|spin|2|0%|1\
         |3|50% { opacity: 0.5; }|2|100%"
    );
}

#[test]
fn fetch_via_script_fetcher() {
    use blitz_vibey_script::{FetchError, ScriptFetcher};
    use url::Url;

    struct MapFetcher;
    impl ScriptFetcher for MapFetcher {
        fn fetch(&self, url: &Url) -> Result<String, FetchError> {
            match url.path() {
                "/data.json" => Ok(r#"{"answer": 42}"#.to_string()),
                _ => Err(FetchError::Io(std::io::Error::from(
                    std::io::ErrorKind::NotFound,
                ))),
            }
        }
    }

    let mut doc = ScriptDocument::from_html(
        r#"
        <html><body>
            <div id="out"></div>
            <div id="missing"></div>
            <script>
                fetch("/data.json")
                    .then((r) => { if (!r.ok) throw new Error("not ok"); return r.json(); })
                    .then((json) => {
                        document.getElementById("out").textContent = String(json.answer);
                    });
                fetch("missing.txt").then((r) => {
                    document.getElementById("missing").textContent =
                        r.status + " " + r.ok + " " + r.url;
                });
            </script>
        </body></html>
        "#,
        DocumentConfig {
            base_url: Some("http://example.test/dir/page.html".to_string()),
            ..Default::default()
        },
    )
    .with_fetcher(MapFetcher);
    doc.execute_scripts();
    assert_eq!(doc.take_js_errors(), Vec::<String>::new());
    assert_eq!(text_of_selector(&doc, "#out"), "42");
    assert_eq!(
        text_of_selector(&doc, "#missing"),
        "404 false http://example.test/dir/missing.txt"
    );
}

#[test]
fn uncaught_errors_fire_window_error_event() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="out"></div>
            <script>
                window.addEventListener("error", (e) => {
                    document.getElementById("out").textContent +=
                        "[" + e.type + ":" + e.message + ":" + (e.error instanceof TypeError) + "]";
                });
                window.onerror = (message) => {
                    document.getElementById("out").textContent += "[onerror:" + message + "]";
                    throw new Error("from handler");
                };
            </script>
            <script>null.foo;</script>
            <script>document.getElementById("out").textContent += "[after]";</script>
        </body></html>
        "#,
    );
    doc.execute_scripts();
    let errors = doc.take_js_errors();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].contains("TypeError"), "{errors:?}");
    assert!(errors[1].contains("from handler"), "{errors:?}");
    let out = text_of_selector(&doc, "#out");
    assert!(
        out.starts_with("[error:") && out.contains(":true][onerror:") && out.ends_with("[after]"),
        "{out}"
    );
}
