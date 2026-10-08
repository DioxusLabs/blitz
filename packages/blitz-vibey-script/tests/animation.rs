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
    // The `finished` reaction runs in the microtask checkpoint before events are dispatched.
    assert_eq!(out, "removed,active,1,persisted,2,remove,persisted,active");
}

#[test]
fn commit_styles() {
    let out = run(r#"
        const div = document.createElement('div');
        document.body.appendChild(div);
        div.style.opacity = '0.1';
        const a = div.animate({ opacity: 0.2 }, { duration: 1, fill: 'forwards' });
        a.finish();
        a.commitStyles();
        a.cancel();
        done([div.style.opacity, getComputedStyle(div).opacity]);
    "#);
    assert_eq!(out, "0.2,0.2");
}

#[test]
fn removing_element_cancels_css_animation() {
    let out = run(r#"
        const style = document.createElement("style");
        style.textContent = "@keyframes a { from { margin-left: 0px } to { margin-left: 100px } }";
        document.head.appendChild(style);
        const div = document.createElement("div");
        div.setAttribute("style", "animation: a 100s");
        document.body.appendChild(div);
        const events = [];
        for (const n of ["animationstart", "animationcancel"]) div.addEventListener(n, e => events.push(n));
        getComputedStyle(div).marginLeft;
        await new Promise(requestAnimationFrame);
        await new Promise(requestAnimationFrame);
        div.remove();
        events.push("removed");
        await new Promise(requestAnimationFrame);
        await new Promise(requestAnimationFrame);
        done(events);
    "#);
    assert_eq!(out, "animationstart,removed,animationcancel");
}

#[test]
fn paused_transition_keeps_current_time() {
    let out = run(r#"
        box.style.transition = "width 1s linear";
        await new Promise(requestAnimationFrame);
        await new Promise(requestAnimationFrame);
        box.style.width = "200px";
        const log = [];
        const promises = [];
        document.getAnimations().forEach(anim => { anim.pause(); anim.currentTime = 500; promises.push(anim.ready); });
        log.push(promises.length);
        setTimeout(() => log.push("timer " + getComputedStyle(box).width + " " + document.getAnimations().length), 5000);
        await Promise.all(promises);
        log.push(getComputedStyle(box).width);
        done(log);
    "#);
    assert_eq!(out, "1,150px");
}

#[test]
fn display_none_cancels_transitions() {
    let out = run(r#"
        const log = [];
        box.style.transition = "width 0.4s";
        getComputedStyle(box).width;
        box.style.width = "200px";
        for (const n of ["transitionrun", "transitionend", "transitioncancel"]) box.addEventListener(n, e => log.push(n));
        await new Promise(requestAnimationFrame);
        log.push("anims " + box.getAnimations().length);
        box.style.display = "none";
        await new Promise(r => setTimeout(r, 500));
        done(log);
    "#);
    assert_eq!(out, "transitionrun,anims 1,transitioncancel");
}

#[test]
fn animate_does_not_start_transition() {
    let out = run(r#"
        const log = [];
        box.addEventListener("transitionrun", () => log.push("transitionrun"));
        box.style.transition = "opacity 100s";
        getComputedStyle(box).opacity;
        box.style.opacity = "0.5";
        const anim = box.animate({ opacity: [0, 1] }, 100000);
        await anim.ready;
        await new Promise(requestAnimationFrame);
        log.push(box.getAnimations().length);
        done(log);
    "#);
    assert_eq!(out, "1");
}

#[test]
fn display_transition_survives_display_none() {
    let out = run(r#"
        const log = [];
        getComputedStyle(box).display;
        box.style.transitionDuration = "100s";
        box.style.transitionDelay = "-10s";
        box.style.transitionTimingFunction = "linear";
        box.style.transitionProperty = "display";
        box.style.transitionBehavior = "allow-discrete";
        box.style.display = "none";
        log.push(getComputedStyle(box).display);
        log.push(box.getAnimations().length);
        log.push(getComputedStyle(box).display);
        done(log);
    "#);
    assert_eq!(out, "block,1,block");
}

#[test]
fn display_transition_ends_without_timers() {
    let out = run(r#"
        const log = [];
        box.style.transitionBehavior = "allow-discrete";
        box.style.transitionDuration = "0.01s";
        getComputedStyle(box).display;
        box.style.display = "none";
        log.push(getComputedStyle(box).display);
        box.addEventListener("transitionend", () => {
            log.push(getComputedStyle(box).display);
            done(log);
        });
    "#);
    assert_eq!(out, "block,none");
}

#[test]
fn unsupported_pseudo_element_does_not_animate_element() {
    let out = run(r#"
        box.animate({ opacity: [0.5, 0.5] }, { pseudoElement: "::marker", duration: Infinity });
        done(getComputedStyle(box).opacity);
    "#);
    assert_eq!(out, "1");
}

#[test]
fn revert_in_keyframe_uses_user_agent_value() {
    let out = run(r#"
        const h1 = document.createElement("h1");
        document.body.append(h1);
        const expected = getComputedStyle(h1).marginTop;
        h1.style.marginTop = "0px";
        h1.animate({ marginTop: ["revert", "revert"] }, { duration: Infinity });
        done(expected !== "0px" && getComputedStyle(h1).marginTop === expected);
    "#);
    assert_eq!(out, "true");
}

#[test]
fn removal_cancels_paused_css_animation_without_timers() {
    let out = run(r#"
        const style = document.createElement("style");
        style.textContent = "@keyframes testAnim { from { margin-left: 0px } to { margin-left: 100px } }";
        document.head.append(style);
        const log = [];
        const div = document.createElement("div");
        div.setAttribute("style", "animation: testAnim 100s paused");
        for (const n of ["animationstart", "animationcancel"]) div.addEventListener(n, () => log.push(n));
        document.body.append(div);
        await new Promise(r => div.addEventListener("animationstart", r));
        log.push("started");
        div.remove();
        await new Promise(r => div.addEventListener("animationcancel", r));
        done(log);
    "#);
    assert_eq!(out, "animationstart,started,animationcancel");
}

#[test]
fn moving_element_cancels_css_animation() {
    let out = run(r#"
        const style = document.createElement("style");
        style.textContent = "@keyframes testAnim { from { margin-left: 0px } to { margin-left: 100px } }";
        document.head.append(style);
        const log = [];
        const container = document.createElement("div");
        document.body.append(container);
        const div = document.createElement("div");
        div.setAttribute("style", "animation: testAnim 100s paused");
        for (const n of ["animationstart", "animationcancel"]) div.addEventListener(n, () => log.push(n));
        document.body.append(div);
        await new Promise(r => div.addEventListener("animationstart", r));
        container.append(div);
        await new Promise(r => div.addEventListener("animationcancel", r));
        const anim = div.getAnimations()[0];
        anim.oncancel = () => log.push("cancel");
        div.remove();
        await new Promise(r => setTimeout(r, 100));
        done(log);
    "#);
    assert_eq!(
        out,
        "animationstart,animationcancel,animationstart,cancel,animationcancel"
    );
}

#[test]
fn animation_started_in_frame_callback_starts_at_frame_time() {
    let out = run(r#"
        const frameTime = await new Promise(requestAnimationFrame);
        const inFrame = box.animate(null, 100000);
        await inFrame.ready;
        await new Promise(r => setTimeout(r, 5));
        const betweenFrames = box.animate(null, 100000);
        const nextFrameTime = await new Promise(requestAnimationFrame);
        await betweenFrames.ready;
        done([frameTime, inFrame.startTime, nextFrameTime, betweenFrames.startTime]);
    "#);
    assert_eq!(out, "16,16,32,32");
}

#[test]
fn reversed_animation_finishes_in_delay() {
    let out = run(r#"
        const animation = box.animate({ opacity: [0, 1] }, { duration: 1000, delay: 50 });
        await animation.ready;
        animation.currentTime = 100;
        animation.reverse();
        await animation.finished;
        done(animation.currentTime);
    "#);
    assert_eq!(out, "0");
}

#[test]
fn settling_unobserved_ready_promise_does_not_get_then() {
    let out = run(r#"
        const log = [];
        const anim = new Animation();
        let resolveFinished;
        const thenCalled = new Promise(resolve => {
            Object.defineProperty(anim, "then", { get() {
                log.push('get');
                return (resolveAnim) => { resolveFinished = resolveAnim; resolve(); };
            } });
        });
        const finished = anim.finished;
        anim.finish();
        anim.cancel();
        await thenCalled;
        resolveFinished('hello');
        log.push(await finished);
        done(log);
    "#);
    assert_eq!(out, "get,hello");
}
