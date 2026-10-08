//! Tests for the Web Animations bindings

use blitz_dom::{Document, DocumentConfig};
use blitz_vibey_script::ScriptDocument;

/// Runs `script` and then timers (in virtual time) until `#out` has text.
fn run(script: &str) -> String {
    let html = format!(
        r#"<html><body><div id="box" style="width: 100px; opacity: 1"></div><div id="out"></div>
        <script>
            const box = document.getElementById("box");
            const done = (value) => document.getElementById("out").textContent = String(value);
            (async () => {{ {script} }})().catch((error) => done("error: " + error));
        </script></body></html>"#
    );
    let mut doc = ScriptDocument::from_html(&html, DocumentConfig::default()).with_virtual_time();
    doc.execute_scripts();
    for _ in 0..1000 {
        let out = {
            let inner = doc.inner();
            let node_id = inner.query_selector("#out").unwrap().unwrap();
            inner.get_node(node_id).unwrap().text_content()
        };
        if !out.is_empty() {
            return out;
        }
        let Some(deadline) = doc.next_timer_deadline() else {
            break;
        };
        doc.advance_clock_to(deadline);
        doc.poll(None);
    }
    panic!("the script did not finish");
}

#[test]
fn animate_plays_and_becomes_ready() {
    let out = run(r#"
        const animation = box.animate({ opacity: [0, 1] }, 1000);
        const before = [animation.playState, animation.pending, animation.startTime];
        await animation.ready;
        done([...before, animation.playState, animation.pending,
              animation.startTime === document.timeline.currentTime]);
    "#);
    assert_eq!(out, "running,true,,running,false,true");
}

#[test]
fn seeking_changes_computed_style() {
    let out = run(r#"
        const animation = box.animate({ width: ["100px", "200px"] }, 1000);
        animation.pause();
        animation.currentTime = 500;
        const half = getComputedStyle(box).width;
        animation.currentTime = 250;
        const quarter = getComputedStyle(box).width;
        animation.cancel();
        done([half, quarter, getComputedStyle(box).width]);
    "#);
    assert_eq!(out, "150px,125px,100px");
}

#[test]
fn finishes_with_event_and_promise() {
    let out = run(r#"
        const animation = box.animate({ opacity: [0, 1] }, 100);
        const events = [];
        animation.onfinish = (event) => events.push(event.type + ":" + event.currentTime);
        await animation.finished;
        await new Promise(requestAnimationFrame);
        done([animation.playState, animation.currentTime, ...events]);
    "#);
    assert_eq!(out, "finished,100,finish:100");
}

#[test]
fn get_animations_and_composite_add() {
    let out = run(r#"
        const a = box.animate({ width: ["200px", "200px"] }, { duration: 1000, fill: "both" });
        const b = box.animate({ width: ["50px", "50px"] }, { duration: 1000, composite: "add" });
        const list = box.getAnimations();
        const width = getComputedStyle(box).width;
        b.cancel();
        done([list.length, list[0] === a, list[1] === b, width,
              document.getAnimations().length, getComputedStyle(box).width]);
    "#);
    assert_eq!(out, "2,true,true,250px,1,200px");
}

#[test]
fn css_animations_are_exposed() {
    let out = run(r#"
        const style = document.createElement("style");
        style.textContent = "@keyframes grow { to { width: 300px } } #box { animation: grow 1s linear paused }";
        document.head.appendChild(style);
        getComputedStyle(box).width;
        const [animation] = box.getAnimations();
        const info = [animation instanceof CSSAnimation, animation.animationName, animation.playState];
        animation.currentTime = 500;
        done([...info, getComputedStyle(box).width]);
    "#);
    assert_eq!(out, "true,grow,paused,200px");
}

#[test]
fn css_animation_events_and_timing() {
    let out = run(r#"
        const style = document.createElement('style');
        style.textContent = '@keyframes anim { from { margin-left: 0px } to { margin-left: 100px } }';
        document.head.appendChild(style);
        const log = [];
        for (const n of ['animationstart','animationend','animationcancel','animationiteration']) box.addEventListener(n, (e) => log.push(n + ':' + e.elapsedTime));
        box.style.animation = 'anim 100s';
        const a = box.getAnimations()[0];
        log.push(a.effect.getTiming().duration);
        await new Promise(requestAnimationFrame);
        log.push('f1');
        await new Promise(requestAnimationFrame);
        log.push('f2');
        await new Promise(requestAnimationFrame);
        a.finish();
        await new Promise(requestAnimationFrame);
        await new Promise(requestAnimationFrame);
        done(log);
    "#);
    assert_eq!(out, "100000,animationstart:0,f1,f2,animationend:100");
}

#[test]
fn replaced_animations_are_removed() {
    let out = run(r#"
        const a = box.animate({ opacity: 0.2 }, { duration: 1, fill: 'forwards' });
        const b = box.animate({ opacity: 0.3 }, { duration: 1, fill: 'forwards' });
        const log = [];
        a.onremove = () => log.push('remove');
        await a.finished;
        log.push(a.replaceState, b.replaceState, box.getAnimations().length);
        a.persist();
        log.push(a.replaceState, box.getAnimations().length);
        await new Promise(requestAnimationFrame);
        await new Promise(requestAnimationFrame);
        log.push(a.replaceState, b.replaceState);
        done(log);
    "#);
    assert_eq!(out, "remove,removed,active,1,persisted,2,persisted,active");
}
