//! IME default actions generate ordered composition events for HTML and Dioxus inputs.

use blitz_test_harness::Harness;
use blitz_traits::events::{
    BlitzCompositionEvent, BlitzImeEvent, DomEventData, DomEventKind, UiEvent,
};
use dioxus::prelude::*;
use std::str::FromStr;

fn input_harness() -> Harness {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="text" type="text" style="width:200px; height:20px;">
        </body></html>"#,
    );
    harness.click("#text");
    assert_eq!(harness.focused(), Some(harness.node("#text")));
    harness
}

fn preedit(text: &str) -> BlitzImeEvent {
    BlitzImeEvent::Preedit(text.into(), Some((text.len(), text.len())))
}

fn commit(text: &str) -> BlitzImeEvent {
    BlitzImeEvent::Commit(text.into())
}

fn record(harness: &mut Harness, events: impl IntoIterator<Item = BlitzImeEvent>) -> Vec<String> {
    harness
        .dispatch_recorded(events.into_iter().map(UiEvent::Ime))
        .into_iter()
        .filter(|name| {
            matches!(
                name.as_str(),
                "compositionstart" | "compositionupdate" | "compositionend" | "input"
            )
        })
        .collect()
}

fn assert_value<D: blitz_dom::Document>(harness: &Harness<D>, expected: &str) {
    let doc = harness.base();
    let element = doc
        .get_node(harness.node("#text"))
        .unwrap()
        .element_data()
        .unwrap();
    assert_eq!(element.text_input_data().unwrap().editor.text(), expected);
}

#[test]
fn korean_preedit_and_commit_are_ordered() {
    let mut harness = input_harness();
    assert_eq!(
        record(
            &mut harness,
            [
                BlitzImeEvent::Enabled,
                preedit("ㅎ"),
                preedit("하"),
                preedit("한"),
                preedit(""),
                commit("한"),
            ],
        ),
        [
            "compositionstart",
            "compositionupdate",
            "compositionupdate",
            "compositionupdate",
            "compositionupdate",
            "input",
            "compositionend",
        ]
    );
    assert_value(&harness, "한");
}

#[test]
fn commit_without_empty_preedit() {
    let mut harness = input_harness();
    assert_eq!(
        record(&mut harness, [preedit("가"), commit("가")]),
        [
            "compositionstart",
            "compositionupdate",
            "compositionupdate",
            "input",
            "compositionend",
        ]
    );
    assert_value(&harness, "가");
}

#[test]
fn disabled_ends_composition() {
    let mut harness = input_harness();
    assert_eq!(
        record(&mut harness, [preedit("a"), BlitzImeEvent::Disabled]),
        ["compositionstart", "compositionupdate", "compositionend"]
    );
    assert_value(&harness, "");
    assert_eq!(record(&mut harness, [commit("x")]), ["input"]);
    assert_value(&harness, "x");
}

#[test]
fn direct_commit_only_generates_input() {
    let mut harness = input_harness();
    assert_eq!(record(&mut harness, [commit("x")]), ["input"]);
    assert_value(&harness, "x");
}

#[test]
fn separately_committed_syllables_have_separate_cycles() {
    let mut harness = input_harness();
    assert_eq!(
        record(
            &mut harness,
            [
                preedit("한"),
                preedit(""),
                commit("한"),
                preedit("글"),
                commit("글")
            ],
        ),
        [
            "compositionstart",
            "compositionupdate",
            "compositionupdate",
            "input",
            "compositionend",
            "compositionstart",
            "compositionupdate",
            "compositionupdate",
            "input",
            "compositionend",
        ]
    );
    assert_value(&harness, "한글");
}

#[test]
fn empty_preedit_defers_end_until_restart_or_disabled() {
    let mut harness = input_harness();
    assert_eq!(
        record(&mut harness, [preedit("a"), preedit(""), preedit("")]),
        ["compositionstart", "compositionupdate"]
    );
    assert_eq!(
        record(&mut harness, [preedit("b")]),
        ["compositionend", "compositionstart", "compositionupdate"]
    );
    assert_eq!(
        record(&mut harness, [preedit(""), BlitzImeEvent::Disabled]),
        ["compositionend"]
    );
    assert_value(&harness, "");
    assert!(record(&mut harness, [BlitzImeEvent::Disabled, preedit("")]).is_empty());
}

#[test]
fn enabled_and_delete_surrounding_do_not_end_composition() {
    let mut harness = input_harness();
    record(&mut harness, [preedit("a")]);
    assert!(
        record(
            &mut harness,
            [
                BlitzImeEvent::Enabled,
                BlitzImeEvent::DeleteSurrounding {
                    before_bytes: 0,
                    after_bytes: 0,
                },
            ],
        )
        .is_empty()
    );
    assert_eq!(
        record(&mut harness, [commit("a")]),
        ["compositionupdate", "input", "compositionend"]
    );
    assert_value(&harness, "a");
}

#[test]
fn composition_event_metadata() {
    let data = BlitzCompositionEvent { data: "한".into() };
    for (event, name, cancelable) in [
        (
            DomEventData::CompositionStart(data.clone()),
            "compositionstart",
            true,
        ),
        (
            DomEventData::CompositionUpdate(data.clone()),
            "compositionupdate",
            false,
        ),
        (DomEventData::CompositionEnd(data), "compositionend", false),
    ] {
        assert_eq!(event.name(), name);
        assert_eq!(event.kind(), DomEventKind::from_str(name).unwrap());
        assert_eq!(
            event.kind(),
            DomEventKind::from_str(&format!("on{name}")).unwrap()
        );
        assert_eq!(event.discriminant(), event.kind().discriminant());
        assert_eq!(event.cancelable(), cancelable);
        assert!(event.bubbles());
    }
    assert_eq!(
        DomEventData::Ime(BlitzImeEvent::Enabled).name(),
        "composition"
    );
    assert_eq!(DomEventKind::from_str("composition"), Ok(DomEventKind::Ime));
}

fn composition_app() -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        // Ancestor listeners also exercise bubbling and listener-count registration.
        div {
            oncompositionstart: move |evt| {
                evt.prevent_default();
                log.write().push(format!("start:{}", evt.data().data()));
            },
            oncompositionupdate: move |evt| log.write().push(format!("update:{}", evt.data().data())),
            oncompositionend: move |evt| log.write().push(format!("end:{}", evt.data().data())),
            oninput: move |evt| log.write().push(format!("input:{}", evt.value())),
            input { id: "text", r#type: "text", style: "width:200px; height:20px;" }
        }
        div { id: "log", "{log.read().join(\"|\")}" }
    }
}

#[test]
fn dioxus_receives_data_in_order_despite_cancelled_start() {
    let mut harness = Harness::from_component(composition_app);
    harness.click("#text");
    for event in [
        BlitzImeEvent::Enabled,
        preedit("ㅎ"),
        preedit("하"),
        preedit("한"),
        preedit(""),
        commit("한"),
        preedit("글"),
        commit("글"),
    ] {
        harness.ime(event);
    }
    assert_eq!(
        harness.text_content("#log"),
        "start:|update:ㅎ|update:하|update:한|update:한|input:한|end:한|start:|update:글|update:글|input:한글|end:글"
    );
    assert_value(&harness, "한글");
}

#[test]
fn dioxus_cancel_and_restart_have_empty_end_data() {
    let mut harness = Harness::from_component(composition_app);
    harness.click("#text");
    for event in [
        preedit("a"),
        preedit(""),
        preedit("b"),
        BlitzImeEvent::Disabled,
    ] {
        harness.ime(event);
    }
    assert_eq!(
        harness.text_content("#log"),
        "start:|update:a|end:|start:|update:b|end:"
    );
    assert_value(&harness, "");
}

fn keydown_app() -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        input {
            id: "text",
            r#type: "text",
            style: "width:200px; height:20px;",
            onkeydown: move |evt| log.write().push(format!("keydown:{}", evt.is_composing())),
            onkeyup: move |evt| log.write().push(format!("keyup:{}", evt.is_composing())),
        }
        div { id: "log", "{log.read().join(\"|\")}" }
    }
}

#[test]
fn key_events_report_is_composing_during_composition() {
    use blitz_test_harness::key_event;
    use blitz_traits::events::KeyState;
    use keyboard_types::{Key, Modifiers};

    let down = || UiEvent::KeyDown(key_event(Key::Shift, KeyState::Pressed, Modifiers::empty()));
    let up = || {
        UiEvent::KeyUp(key_event(
            Key::Shift,
            KeyState::Released,
            Modifiers::empty(),
        ))
    };

    let mut harness = Harness::from_component(keydown_app);
    harness.click("#text");
    harness.dispatch(down());
    harness.ime(preedit("ㅎ"));
    harness.dispatch(up());
    harness.dispatch(down());
    harness.ime(preedit(""));
    harness.ime(commit("ㅎ"));
    harness.dispatch(up());
    harness.pump();
    assert_eq!(
        harness.text_content("#log"),
        "keydown:false|keyup:true|keydown:true|keyup:false"
    );

    // A value already set by the shell is kept
    let mut composing_down = key_event(Key::Shift, KeyState::Pressed, Modifiers::empty());
    composing_down.is_composing = true;
    harness.dispatch(UiEvent::KeyDown(composing_down));
    harness.pump();
    assert!(harness.text_content("#log").ends_with("|keydown:true"));
}
