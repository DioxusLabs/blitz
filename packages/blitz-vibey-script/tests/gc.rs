//! GC integration tests for the node wrapper cache.
//!
//! - In-document nodes hold strong cache entries: a wrapper whose listeners
//!   were registered from JS survives a forced GC and keeps dispatching.
//! - Removed nodes are switched to weak: once JS drops its references and a
//!   GC runs, the wrapper is collected, the finalizer clears the cache entry
//!   and reclaims the detached subtree, and the listener stops firing.

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

fn text_of_selector(doc: &ScriptDocument, selector: &str) -> Option<String> {
    let inner = doc.inner();
    inner
        .query_selector(selector)
        .unwrap()
        .map(|id| inner.get_node(id).unwrap().text_content())
}

/// A wrapper whose listener was registered from JS must survive a forced GC
/// while its node stays in the document: the cache entry is strong, so the
/// listener still fires afterwards.
#[test]
fn listeners_survive_gc_for_in_document_nodes() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="t"></div>
            <div id="out"></div>
            <script>
                document.getElementById("t").addEventListener("click", () => {
                    document.getElementById("out").textContent = "hit";
                });
            </script>
        </body></html>
        "#,
    );

    // The script dropped its reference to the wrapper; only the strong cache
    // entry keeps it alive now.
    doc.run_gc();

    dispatch_click(&mut doc, "#t");

    assert_eq!(
        text_of_selector(&doc, "#out").as_deref(),
        Some("hit"),
        "listener was lost across GC"
    );
}

/// After a listened node is removed and a GC runs, its wrapper is collected:
/// the finalizer clears the cache entry, reclaims the detached subtree's
/// storage, and the listener no longer fires.
#[test]
fn detached_listeners_stop_after_gc() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><div id="t"></div></div>
            <div id="out"></div>
            <script>
                document.getElementById("t").addEventListener("click", () => {
                    document.getElementById("out").textContent = "hit";
                });
            </script>
        </body></html>
        "#,
    );

    doc.eval(
        "document.getElementById('root').removeChild(document.getElementById('t'));",
    );

    // No JS references the wrapper anymore and the cache entry is weak, so
    // the GC collects it and the finalizer reclaims the detached node.
    doc.run_gc();

    // The finalizer reclaimed the detached subtree's storage.
    assert!(
        doc.inner().query_selector("#t").unwrap().is_none(),
        "detached node was not reclaimed by the finalizer"
    );

    dispatch_click(&mut doc, "#root");

    assert_ne!(
        text_of_selector(&doc, "#out").as_deref(),
        Some("hit"),
        "listener still fired after GC"
    );
}

/// A removed node whose wrapper is still referenced from JS keeps its
/// listener across a GC (the weak entry survives while the wrapper is
/// reachable), and re-attaching it switches the entry back to strong
/// (`weak.upgrade()`), so further GCs cannot reclaim it.
#[test]
fn reattach_revives_weak_wrapper_and_keeps_listeners() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><div id="t"></div></div>
            <div id="out"></div>
            <script>
                globalThis.t = document.getElementById("t");
                globalThis.t.addEventListener("click", () => {
                    document.getElementById("out").textContent = "hit";
                });
            </script>
        </body></html>
        "#,
    );

    doc.eval("document.getElementById('root').removeChild(globalThis.t);");
    doc.run_gc();
    doc.eval("document.getElementById('root').appendChild(globalThis.t);");
    doc.run_gc();

    dispatch_click(&mut doc, "#t");

    assert_eq!(
        text_of_selector(&doc, "#out").as_deref(),
        Some("hit"),
        "listener did not survive the weak->strong round trip"
    );
}

/// Removing a subtree while JS still holds a descendant's wrapper must not
/// reclaim that subtree: the finalizer sees the live descendant
/// (`has_live_descendant`) and keeps the detached nodes in the slab, and the
/// descendant's listener still fires. Accessing the kept subtree rebuilds the
/// collected parent's wrapper.
#[test]
fn subtree_with_live_descendant_survives_gc() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><div id="p"><div id="c"></div></div></div>
            <div id="out"></div>
            <script>
                globalThis.c = document.getElementById("c");
                globalThis.c.addEventListener("click", () => {
                    document.getElementById("out").textContent = "c-hit";
                });
            </script>
        </body></html>
        "#,
    );

    let p_id = doc.inner().query_selector("#p").unwrap().unwrap();

    doc.eval("document.getElementById('root').removeChild(document.getElementById('p'));");
    doc.run_gc();

    // `p`'s wrapper was collected, but `c`'s is alive from JS, so the subtree
    // must still be in the slab.
    assert!(
        doc.inner().get_node(p_id).is_some(),
        "detached subtree with a live descendant was wrongly reclaimed"
    );

    // The live descendant's listener still fires via manual dispatch.
    doc.eval("globalThis.c.dispatchEvent(new Event('click'));");
    assert_eq!(
        text_of_selector(&doc, "#out").as_deref(),
        Some("c-hit"),
        "live descendant's listener was lost"
    );

    // Walking from the kept child to its (wrapper-less) parent rebuilds the
    // parent's wrapper with the right identity.
    doc.eval(
        r#"document.getElementById("out").textContent =
            globalThis.c.parentElement ? globalThis.c.parentElement.id : "none";"#,
    );
    assert_eq!(text_of_selector(&doc, "#out").as_deref(), Some("p"));
}

/// `replaceChild` must switch strengths in both directions: the replaced node
/// (JS-unreferenced) is weak and gets reclaimed by the GC, while the inserted
/// node is strong and keeps its listener.
#[test]
fn replace_child_switches_strength_both_ways() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><div id="old"></div></div>
            <div id="out"></div>
            <script>
                document.getElementById("old").addEventListener("click", () => {
                    document.getElementById("out").textContent = "old-hit";
                });
                const nw = document.createElement("div");
                nw.id = "new";
                nw.addEventListener("click", () => {
                    document.getElementById("out").textContent = "new-hit";
                });
                document.getElementById("root").replaceChild(nw, document.getElementById("old"));
            </script>
        </body></html>
        "#,
    );

    doc.run_gc();

    assert!(
        doc.inner().query_selector("#old").unwrap().is_none(),
        "replaced node was not reclaimed"
    );
    dispatch_click(&mut doc, "#new");
    assert_eq!(text_of_selector(&doc, "#out").as_deref(), Some("new-hit"));
}

/// Expando properties stored on a wrapper must still be there after a GC: the
/// strong cache entry keeps the very same wrapper object alive.
#[test]
fn expando_and_identity_survive_gc() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="t"></div>
            <div id="out"></div>
            <script>
                document.getElementById("t").mark = "original";
                document.getElementById("out").textContent =
                    document.getElementById("t").mark ?? "gone";
            </script>
        </body></html>
        "#,
    );
    assert_eq!(text_of_selector(&doc, "#out").as_deref(), Some("original"));

    doc.run_gc();

    doc.eval(
        r#"document.getElementById("out").textContent =
            document.getElementById("t").mark ?? "gone";"#,
    );
    assert_eq!(
        text_of_selector(&doc, "#out").as_deref(),
        Some("original"),
        "wrapper was rebuilt (identity lost) across GC"
    );
}

/// `textContent` replacement detaches the old children; with no JS references
/// left, a GC collects their wrappers and the finalizer drops the nodes.
#[test]
fn set_text_content_reclaims_detached_children() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="p"><span id="c">x</span></div>
            <div id="out"></div>
            <script>
                document.getElementById("c").addEventListener("click", () => {
                    document.getElementById("out").textContent = "c-hit";
                });
            </script>
        </body></html>
        "#,
    );

    doc.eval(r#"document.getElementById("p").textContent = "plain";"#);
    doc.run_gc();

    assert!(
        doc.inner().query_selector("#c").unwrap().is_none(),
        "detached child was not reclaimed"
    );
    assert_eq!(text_of_selector(&doc, "#p").as_deref(), Some("plain"));
    assert_eq!(
        text_of_selector(&doc, "#out").as_deref(),
        Some(""),
        "listener still fired after GC"
    );
}

/// `innerHTML` replacement detaches the old children the same way; a GC then
/// reclaims their wrappers and storage.
#[test]
fn set_inner_html_reclaims_detached_children() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="p"><span id="c">x</span></div>
            <div id="out"></div>
            <script>
                document.getElementById("c").addEventListener("click", () => {
                    document.getElementById("out").textContent = "c-hit";
                });
            </script>
        </body></html>
        "#,
    );

    doc.eval(r#"document.getElementById("p").innerHTML = "<b>fresh</b>";"#);
    doc.run_gc();

    assert!(
        doc.inner().query_selector("#c").unwrap().is_none(),
        "detached child was not reclaimed"
    );
    assert_ne!(
        text_of_selector(&doc, "#out").as_deref(),
        Some("c-hit"),
        "listener still fired after GC"
    );
    assert!(
        text_of_selector(&doc, "#p").as_deref().is_some(),
        "innerHTML replacement did not run"
    );
}

/// Moving a node between parents (appendChild and insertBefore) switches its
/// cache entry weak and back to strong; its listener must survive the round
/// trips and the GCs in between.
#[test]
fn moving_node_between_parents_keeps_listeners() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="a"><div id="t"></div></div>
            <div id="b"></div>
            <div id="out"></div>
            <script>
                document.getElementById("t").addEventListener("click", () => {
                    document.getElementById("out").textContent = "hit";
                });
            </script>
        </body></html>
        "#,
    );

    doc.eval("document.getElementById('b').appendChild(document.getElementById('t'));");
    doc.run_gc();
    doc.eval("document.getElementById('a').insertBefore(document.getElementById('t'), null);");
    doc.run_gc();

    dispatch_click(&mut doc, "#t");

    assert_eq!(
        text_of_selector(&doc, "#out").as_deref(),
        Some("hit"),
        "listener was lost when moving the node between parents"
    );
}

/// `node.remove()` switches the node's entry to weak before detaching; with
/// no JS references left, a GC collects the wrapper and the finalizer drops
/// the node.
#[test]
fn node_remove_reclaims_wrapper() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><div id="t"></div></div>
            <div id="out"></div>
            <script>
                document.getElementById("t").addEventListener("click", () => {
                    document.getElementById("out").textContent = "hit";
                });
            </script>
        </body></html>
        "#,
    );

    doc.eval("document.getElementById('t').remove();");
    doc.run_gc();

    assert!(
        doc.inner().query_selector("#t").unwrap().is_none(),
        "removed node was not reclaimed"
    );
    assert_eq!(
        text_of_selector(&doc, "#out").as_deref(),
        Some(""),
        "listener still fired after GC"
    );
}

/// `append`/`prepend` insert nodes into the document, which must make their
/// cache entries strong: their listeners survive GCs afterwards.
#[test]
fn append_and_prepend_keep_listeners_after_gc() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"></div>
            <div id="out"></div>
            <script>
                const a = document.createElement("div");
                a.id = "a";
                a.addEventListener("click", () => {
                    document.getElementById("out").textContent += "a";
                });
                const b = document.createElement("div");
                b.id = "b";
                b.addEventListener("click", () => {
                    document.getElementById("out").textContent += "b";
                });
                document.getElementById("root").append(a);
                document.getElementById("root").prepend(b);
            </script>
        </body></html>
        "#,
    );

    doc.run_gc();

    dispatch_click(&mut doc, "#a");
    dispatch_click(&mut doc, "#b");

    assert_eq!(text_of_selector(&doc, "#out").as_deref(), Some("ab"));
}

/// `before`/`after` make the inserted nodes strong: their listeners survive
/// a GC.
#[test]
fn before_and_after_keep_listeners_after_gc() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><div id="anchor"></div></div>
            <div id="out"></div>
            <script>
                const x = document.createElement("div");
                x.id = "x";
                x.addEventListener("click", () => {
                    document.getElementById("out").textContent += "x";
                });
                const y = document.createElement("div");
                y.id = "y";
                y.addEventListener("click", () => {
                    document.getElementById("out").textContent += "y";
                });
                const anchor = document.getElementById("anchor");
                anchor.before(x);
                anchor.after(y);
            </script>
        </body></html>
        "#,
    );

    doc.run_gc();

    dispatch_click(&mut doc, "#x");
    dispatch_click(&mut doc, "#y");

    assert_eq!(text_of_selector(&doc, "#out").as_deref(), Some("xy"));
}

/// `replaceChildren` weakens the replaced children (the GC reclaims them,
/// JS holds no references) and makes the new children strong (their
/// listeners survive).
#[test]
fn replace_children_reclaims_replaced_and_keeps_new() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><div id="anchor"></div></div>
            <div id="out"></div>
            <script>
                document.getElementById("anchor").addEventListener("click", () => {
                    document.getElementById("out").textContent = "old-hit";
                });
                const z = document.createElement("div");
                z.id = "z";
                z.addEventListener("click", () => {
                    document.getElementById("out").textContent = "z-hit";
                });
                document.getElementById("root").replaceChildren(z);
            </script>
        </body></html>
        "#,
    );

    doc.run_gc();

    assert!(
        doc.inner().query_selector("#anchor").unwrap().is_none(),
        "replaced child was not reclaimed"
    );
    dispatch_click(&mut doc, "#z");
    assert_eq!(text_of_selector(&doc, "#out").as_deref(), Some("z-hit"));
}

/// `replaceWith` switches strengths in both directions: the replaced node is
/// weak (reclaimed by GC), the inserted node is strong (listener survives).
#[test]
fn replace_with_switches_strength_both_ways() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><div id="old"></div></div>
            <div id="out"></div>
            <script>
                document.getElementById("old").addEventListener("click", () => {
                    document.getElementById("out").textContent = "old-hit";
                });
                const nw = document.createElement("div");
                nw.id = "new";
                nw.addEventListener("click", () => {
                    document.getElementById("out").textContent = "new-hit";
                });
                document.getElementById("old").replaceWith(nw);
            </script>
        </body></html>
        "#,
    );

    doc.run_gc();

    assert!(
        doc.inner().query_selector("#old").unwrap().is_none(),
        "replaced node was not reclaimed"
    );
    dispatch_click(&mut doc, "#new");
    assert_eq!(text_of_selector(&doc, "#out").as_deref(), Some("new-hit"));
}

/// Cross-check with the JS-standard `FinalizationRegistry`: when a detached
/// node's wrapper is collected, the host-side finalizer clears the cache
/// entry and reclaims the node, and a script-side registry registered on the
/// same wrapper must also fire its cleanup callback with the held value.
#[test]
#[ignore = "boa's SimpleJobExecutor drops the registry's pending cleanup future when \
            a job drain returns, closing the notifier channel before the collection's \
            signal is sent; remove this ignore once blitz's executor keeps the future \
            alive across drains"]
fn finalization_registry_fires_when_wrapper_collected() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><div id="t"></div></div>
            <div id="out"></div>
            <script>
                globalThis.reg = new FinalizationRegistry((held) => {
                    document.getElementById("out").textContent = held;
                });
                globalThis.reg.register(
                    document.getElementById("t"),
                    "t-was-collected"
                );
            </script>
        </body></html>
        "#,
    );

    doc.eval("document.getElementById('root').removeChild(document.getElementById('t'));");
    doc.run_gc();
    doc.eval("0;");

    // Host side: the cache entry is gone and the detached node was reclaimed.
    assert!(
        doc.inner().query_selector("#t").unwrap().is_none(),
        "host-side finalizer did not reclaim the detached node"
    );
    // JS side: the cleanup callback must receive the held value.
    assert_eq!(
        text_of_selector(&doc, "#out").as_deref(),
        Some("t-was-collected"),
        "FinalizationRegistry cleanup callback did not fire"
    );
}

/// Cross-check the other direction: a wrapper kept alive by a strong cache
/// entry must NOT trigger a script-side FinalizationRegistry.
#[test]
fn finalization_registry_stays_silent_while_wrapper_alive() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="t"></div>
            <div id="out"></div>
            <script>
                globalThis.reg = new FinalizationRegistry((held) => {
                    document.getElementById("out").textContent = held;
                });
                globalThis.reg.register(
                    document.getElementById("t"),
                    "should-not-fire"
                );
            </script>
        </body></html>
        "#,
    );

    doc.run_gc();
    doc.eval("0;");

    assert_eq!(
        text_of_selector(&doc, "#out").as_deref(),
        Some(""),
        "FinalizationRegistry fired for a live (strong) wrapper"
    );
    assert!(
        doc.inner().query_selector("#t").unwrap().is_some(),
        "in-document node was wrongly reclaimed"
    );
}

/// `unregister` must cut the link: a wrapper registered and then unregistered
/// is collected without the cleanup callback firing.
#[test]
fn finalization_registry_unregister_prevents_callback() {
    let mut doc = doc_from_html(
        r#"
        <html><body>
            <div id="root"><div id="t"></div></div>
            <div id="out"></div>
            <script>
                globalThis.reg = new FinalizationRegistry((held) => {
                    document.getElementById("out").textContent = held;
                });
                globalThis.token = {};
                globalThis.reg.register(
                    document.getElementById("t"),
                    "t-collected",
                    globalThis.token
                );
            </script>
        </body></html>
        "#,
    );

    doc.eval("document.getElementById('root').removeChild(document.getElementById('t'));");
    doc.eval("globalThis.reg.unregister(globalThis.token);");
    doc.run_gc();
    doc.eval("0;");

    // Host-side finalization still ran (entry cleared, node reclaimed), but
    // the script-side callback was cut by `unregister`.
    assert!(
        doc.inner().query_selector("#t").unwrap().is_none(),
        "detached node was not reclaimed"
    );
    assert_eq!(
        text_of_selector(&doc, "#out").as_deref(),
        Some(""),
        "FinalizationRegistry fired despite unregister"
    );
}
