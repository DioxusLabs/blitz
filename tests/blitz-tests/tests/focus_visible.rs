//! `:focus-visible` matches focused elements when keyboard modality is active or
//! when the focused element is a text-entry control, while pointer focus on
//! non-text controls suppresses `:focus-visible` until keyboard or IME input
//! occurs.

use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::{
    Document, EventDriver, EventHandler, FocusOptions, FocusSource, QualName, local_name, ns,
};
use blitz_test_harness::{Harness, HarnessOptions, key_event};
use blitz_traits::events::{BlitzImeEvent, DomEvent, EventState, KeyState, UiEvent};
use dioxus::prelude::*;
use keyboard_types::{Key, Modifiers, NamedKey};

fn style_value<D: Document>(harness: &Harness<D>, selector: &str, property: &str) -> String {
    let id = harness.node(selector);
    harness.base().resolved_style_value(id, property)
}

fn matches_focus_visible<D: Document>(harness: &Harness<D>, id_selector: &str) -> bool {
    harness
        .query(&format!("{id_selector}:focus-visible"))
        .is_some()
}

fn matches_focus<D: Document>(harness: &Harness<D>, id_selector: &str) -> bool {
    harness.query(&format!("{id_selector}:focus")).is_some()
}

#[test]
fn pointer_press_focuses_interactive_ancestors_without_focus_visible() {
    let mut harness = Harness::from_html_with(
        r##"<html><head><style>
            :focus { background-color: rgb(220, 220, 255); }
            :focus-visible { outline: 3px solid rgb(0, 128, 0); }
        </style></head><body style="margin:0">
            <input id="text" type="text" style="display:block; width:120px; height:24px;">
            <button id="button" style="display:block; width:120px; height:24px;"><span id="btn-label">Go</span></button>
            <div id="roving" tabindex="-1" style="width:120px; height:24px;"><span id="item">Item</span></div>
            <a id="link" href="#plain" style="display:block; width:120px; height:24px;">Link</a>
            <div id="plain" style="width:120px; height:24px;">Plain</div>
            <button id="disabled" disabled style="display:block; width:120px; height:24px;">Off</button>
        </body></html>"##,
        HarnessOptions {
            width: 300,
            height: 300,
            ..Default::default()
        },
    );

    harness.click("#btn-label");
    assert_eq!(harness.focused(), Some(harness.node("#button")));
    assert!(matches_focus(&harness, "#button"));
    assert!(!matches_focus_visible(&harness, "#button"));
    assert_eq!(style_value(&harness, "#button", "outline-style"), "none");

    harness.click("#item");
    assert_eq!(harness.focused(), Some(harness.node("#roving")));
    assert!(matches_focus(&harness, "#roving"));
    assert!(!matches_focus_visible(&harness, "#roving"));

    harness.click("#link");
    assert_eq!(harness.focused(), Some(harness.node("#link")));
    assert!(matches_focus(&harness, "#link"));
    assert!(!matches_focus_visible(&harness, "#link"));

    harness.click("#disabled");
    assert_eq!(harness.focused(), Some(harness.node("#link")));

    harness.click("#plain");
    assert_eq!(harness.focused(), Some(harness.node("html")));
    assert!(harness.query(":focus").is_none());
    assert!(harness.query(":focus-visible").is_none());
}

#[test]
fn mouse_moves_focus_on_pointerdown_and_touch_moves_focus_on_tap() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="text" type="text" style="display:block; width:120px; height:24px;">
            <button id="button" style="display:block; width:120px; height:24px;">Press</button>
        </body></html>"#,
    );

    let (bx, by) = harness.center_of("#button");
    harness.mouse_down_at(bx, by);
    assert_eq!(harness.focused(), Some(harness.node("#button")));
    assert!(!matches_focus_visible(&harness, "#button"));
    harness.mouse_up_at(bx, by);

    harness.click("#text");
    assert_eq!(harness.focused(), Some(harness.node("#text")));
    assert!(matches_focus_visible(&harness, "#text"));

    harness.touch_down(1, bx, by);
    assert_eq!(harness.focused(), Some(harness.node("#text")));
    harness.touch_up(1, bx, by);
    assert_eq!(harness.focused(), Some(harness.node("#button")));
    assert!(matches_focus(&harness, "#button"));
    assert!(!matches_focus_visible(&harness, "#button"));
}

#[test]
fn clicking_label_focuses_bound_control_using_pointer_modality() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="text" type="text" style="display:block; width:100px; height:20px;">
            <input id="check" type="checkbox" style="display:block; width:20px; height:20px;">
            <label id="for-text" for="text" style="display:block; width:100px; height:20px;">Text</label>
            <label id="for-check" for="check" style="display:block; width:100px; height:20px;">Check</label>
        </body></html>"#,
    );

    harness.click("#for-text");
    assert_eq!(harness.focused(), Some(harness.node("#text")));
    assert!(matches_focus_visible(&harness, "#text"));

    harness.click("#for-check");
    assert_eq!(harness.focused(), Some(harness.node("#check")));
    assert!(matches_focus(&harness, "#check"));
    assert!(!matches_focus_visible(&harness, "#check"));
}

#[test]
fn tab_and_shift_tab_focus_elements_with_focus_visible() {
    let mut harness = Harness::from_html(
        r##"<html><head><style>
            :focus-visible { background-color: rgb(0, 200, 0); }
        </style></head><body style="margin:0">
            <button id="b1" style="display:block; width:100px; height:24px;">One</button>
            <div id="d2" tabindex="0" style="width:100px; height:24px;">Two</div>
            <a id="a3" href="#b1" style="display:block; width:100px; height:24px;">Three</a>
        </body></html>"##,
    );

    harness.press(Key::Named(NamedKey::Tab));
    assert_eq!(harness.focused(), Some(harness.node("#b1")));
    assert!(matches_focus_visible(&harness, "#b1"));
    assert_eq!(
        style_value(&harness, "#b1", "background-color"),
        "rgb(0, 200, 0)"
    );

    for modifier in [Modifiers::CONTROL, Modifiers::ALT, Modifiers::META] {
        harness.press_with(Key::Named(NamedKey::Tab), modifier);
        assert_eq!(harness.focused(), Some(harness.node("#b1")));
    }

    harness.press(Key::Named(NamedKey::Tab));
    assert_eq!(harness.focused(), Some(harness.node("#d2")));
    assert!(matches_focus_visible(&harness, "#d2"));

    harness.press(Key::Named(NamedKey::Tab));
    assert_eq!(harness.focused(), Some(harness.node("#a3")));
    assert!(matches_focus_visible(&harness, "#a3"));

    harness.press_with(Key::Named(NamedKey::Tab), Modifiers::SHIFT);
    assert_eq!(harness.focused(), Some(harness.node("#d2")));
    assert!(matches_focus_visible(&harness, "#d2"));
}

#[test]
fn focus_visible_selector_validity_and_equal_specificity_with_focus() {
    let harness = Harness::from_html(
        r#"<html><head><style>
            #first:focus-visible { background-color: rgb(255, 0, 0); }
            #first:focus { background-color: rgb(0, 128, 0); }
            #second:focus { background-color: rgb(255, 0, 0); }
            #second:focus-visible { background-color: rgb(0, 128, 0); }
        </style></head><body>
            <button id="first">First</button>
            <button id="second">Second</button>
        </body></html>"#,
    );

    assert!(
        harness
            .base()
            .css_supports_condition("selector(:focus-visible)")
    );

    let mut harness = harness;
    let first = harness.node("#first");
    harness.base_mut().set_focus_to(first);
    harness.pump();
    assert_eq!(
        style_value(&harness, "#first", "background-color"),
        "rgb(0, 128, 0)"
    );

    let second = harness.node("#second");
    harness.base_mut().set_focus_to(second);
    harness.pump();
    assert_eq!(
        style_value(&harness, "#second", "background-color"),
        "rgb(0, 128, 0)"
    );
}

#[test]
fn pointer_click_on_text_entry_inputs_and_textarea_matches_focus_visible() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="implicit" style="display:block; width:80px; height:18px;">
            <input id="text" type="text" style="display:block; width:80px; height:18px;">
            <input id="email" type="email" style="display:block; width:80px; height:18px;">
            <input id="password" type="password" style="display:block; width:80px; height:18px;">
            <input id="search" type="search" style="display:block; width:80px; height:18px;">
            <input id="tel" type="tel" style="display:block; width:80px; height:18px;">
            <input id="url" type="url" style="display:block; width:80px; height:18px;">
            <input id="number" type="number" style="display:block; width:80px; height:18px;">
            <input id="date" type="date" style="display:block; width:80px; height:18px;">
            <input id="dtl" type="datetime-local" style="display:block; width:80px; height:18px;">
            <input id="month" type="month" style="display:block; width:80px; height:18px;">
            <input id="time" type="time" style="display:block; width:80px; height:18px;">
            <input id="week" type="week" style="display:block; width:80px; height:18px;">
            <input id="invalid-type" type="telephone" style="display:block; width:80px; height:18px;">
            <input id="ro" type="text" readonly style="display:block; width:80px; height:18px;">
            <textarea id="area" style="display:block; width:80px; height:24px;"></textarea>
        </body></html>"#,
    );

    for selector in [
        "#implicit",
        "#text",
        "#email",
        "#password",
        "#search",
        "#tel",
        "#url",
        "#number",
        "#date",
        "#dtl",
        "#month",
        "#time",
        "#week",
        "#invalid-type",
        "#ro",
        "#area",
    ] {
        harness.click(selector);
        assert_eq!(
            harness.focused(),
            Some(harness.node(selector)),
            "{selector} should be focused"
        );
        assert!(
            matches_focus_visible(&harness, selector),
            "{selector} should match :focus-visible on pointer click"
        );
        assert_eq!(style_value(&harness, selector, "outline-style"), "solid");
    }
}

#[test]
fn pointer_click_on_non_text_inputs_and_select_suppresses_focus_visible() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="btn" type="button" value="Btn" style="display:block; width:80px; height:20px;">
            <input id="submit" type="submit" value="Sub" style="display:block; width:80px; height:20px;">
            <input id="reset" type="reset" value="Res" style="display:block; width:80px; height:20px;">
            <input id="checkbox" type="checkbox" style="display:block; width:20px; height:20px;">
            <input id="radio" type="radio" style="display:block; width:20px; height:20px;">
            <input id="range" type="range" style="display:block; width:80px; height:20px;">
            <input id="color" type="color" style="display:block; width:80px; height:20px;">
            <input id="image" type="image" style="display:block; width:80px; height:20px;">
            <select id="select" style="display:block; width:80px; height:20px;"><option>A</option></select>
            <input id="file" type="file" style="display:block; width:80px; height:20px;">
            <input id="hidden" type="hidden">
        </body></html>"#,
    );

    for selector in [
        "#btn",
        "#submit",
        "#reset",
        "#checkbox",
        "#radio",
        "#range",
        "#color",
        "#image",
        "#select",
    ] {
        harness.click(selector);
        assert_eq!(
            harness.focused(),
            Some(harness.node(selector)),
            "{selector} should be focused"
        );
        assert!(
            !matches_focus_visible(&harness, selector),
            "{selector} must not match :focus-visible on pointer click"
        );
        assert_eq!(style_value(&harness, selector, "outline-style"), "none");
    }

    let file_id = harness.node("#file");
    harness
        .base_mut()
        .set_focus_to_with_source(file_id, FocusSource::Pointer);
    harness.pump();
    assert!(!matches_focus_visible(&harness, "#file"));

    let select_id = harness.node("#select");
    harness
        .base_mut()
        .set_focus_to_with_source(select_id, FocusSource::Keyboard);
    harness.pump();
    assert!(matches_focus_visible(&harness, "#select"));
    assert_eq!(style_value(&harness, "#select", "outline-style"), "solid");

    let hidden_id = harness.node("#hidden");
    assert!(!harness.base().get_node(hidden_id).unwrap().is_focussable());
}

#[test]
fn contenteditable_always_matches_focus_visible_unlike_role_textbox() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <div id="ce-bare" contenteditable style="width:120px; height:24px;">Edit</div>
            <div id="ce-true" contenteditable="true" style="width:120px; height:24px;">
                <button id="nested-btn" style="display:block; width:60px; height:18px;">Nested</button>
            </div>
            <div id="ce-plain" contenteditable="plaintext-only" style="width:120px; height:24px;">Plain</div>
            <div id="ce-false" contenteditable="false" tabindex="0" style="width:120px; height:24px;">No</div>
            <div id="aria-textbox" role="textbox" tabindex="0" style="width:120px; height:24px;">ARIA</div>
        </body></html>"#,
    );

    for selector in ["#ce-bare", "#ce-true", "#ce-plain"] {
        let id = harness.node(selector);
        assert!(harness.base().get_node(id).unwrap().is_focussable());
    }

    harness.click("#ce-bare");
    assert_eq!(harness.focused(), Some(harness.node("#ce-bare")));
    assert!(matches_focus_visible(&harness, "#ce-bare"));

    harness.click("#ce-plain");
    assert_eq!(harness.focused(), Some(harness.node("#ce-plain")));
    assert!(matches_focus_visible(&harness, "#ce-plain"));

    harness.click("#nested-btn");
    assert_eq!(harness.focused(), Some(harness.node("#nested-btn")));
    assert!(!matches_focus_visible(&harness, "#nested-btn"));

    harness.click("#ce-false");
    assert_eq!(harness.focused(), Some(harness.node("#ce-false")));
    assert!(!matches_focus_visible(&harness, "#ce-false"));

    harness.click("#aria-textbox");
    assert_eq!(harness.focused(), Some(harness.node("#aria-textbox")));
    assert!(!matches_focus_visible(&harness, "#aria-textbox"));

    harness.press_with(Key::Named(NamedKey::Tab), Modifiers::SHIFT);
    harness.press(Key::Named(NamedKey::Tab));
    assert_eq!(harness.focused(), Some(harness.node("#aria-textbox")));
    assert!(matches_focus_visible(&harness, "#aria-textbox"));
}

#[test]
fn ua_default_stylesheet_applies_outline_only_for_focus_visible() {
    let mut harness = Harness::from_html(
        r##"<html><body style="margin:0">
            <div id="box" tabindex="0" style="width:100px; height:20px;">Box</div>
            <button id="btn" style="display:block; width:100px; height:20px;">Btn</button>
            <a id="link" href="#box" style="display:block; width:100px; height:20px;">Link</a>
            <details style="width:100px;"><summary id="sum" style="height:20px;">Summary</summary>Body</details>
        </body></html>"##,
    );

    for selector in ["#box", "#btn", "#link", "#sum"] {
        let id = harness.node(selector);
        harness.base_mut().set_focus_to(id);
        harness.pump();
        assert_ne!(
            style_value(&harness, selector, "outline-style"),
            "none",
            "programmatic focus on {selector} while keyboard modality is active must show UA outline"
        );
    }

    for selector in ["#box", "#btn", "#link", "#sum"] {
        harness.click(selector);
        assert_eq!(
            style_value(&harness, selector, "outline-style"),
            "none",
            "pointer click on {selector} must not show UA outline"
        );
    }
}

#[test]
fn keydown_upgrades_pointer_focused_element_even_when_prevent_default_is_called() {
    #[derive(Clone)]
    struct PreventDefaultOnKeyDown;
    impl EventHandler for PreventDefaultOnKeyDown {
        fn handle_event(
            &mut self,
            _chain: &[blitz_dom::NodeId],
            event: &mut DomEvent,
            _doc: &mut dyn Document,
            event_state: &mut EventState,
        ) {
            if matches!(event.data, blitz_traits::events::DomEventData::KeyDown(_)) {
                event_state.prevent_default();
            }
        }
    }

    let mut harness = Harness::from_html(
        r#"<html><head><style>
            #target:focus-visible { outline: 2px solid rgb(0, 128, 0); }
        </style></head><body style="margin:0">
            <div id="target" tabindex="0" style="width:100px; height:24px;">Target</div>
        </body></html>"#,
    );

    harness.click("#target");
    assert!(matches_focus(&harness, "#target"));
    assert!(!matches_focus_visible(&harness, "#target"));

    {
        let mut doc = harness.base_mut();
        let mut driver = EventDriver::new(&mut *doc, PreventDefaultOnKeyDown);
        driver.handle_ui_event(UiEvent::KeyDown(key_event(
            Key::Named(NamedKey::Enter),
            KeyState::Pressed,
            Modifiers::empty(),
        )));
    }
    harness.pump();

    assert!(matches_focus_visible(&harness, "#target"));
    assert_eq!(
        style_value(&harness, "#target", "outline-color"),
        "rgb(0, 128, 0)"
    );
}

#[test]
fn modifier_keys_and_shortcuts_neither_arm_nor_clear_focus_visible() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <div id="target" tabindex="0" style="width:100px; height:24px;">Target</div>
        </body></html>"#,
    );

    harness.click("#target");
    assert!(!matches_focus_visible(&harness, "#target"));

    for (key, mods) in [
        (Key::Named(NamedKey::Control), Modifiers::CONTROL),
        (Key::Named(NamedKey::Alt), Modifiers::ALT),
        (Key::Named(NamedKey::Meta), Modifiers::META),
        (Key::Named(NamedKey::Shift), Modifiers::SHIFT),
        (Key::Named(NamedKey::CapsLock), Modifiers::CAPS_LOCK),
        (Key::Character("y".into()), Modifiers::CONTROL),
        (Key::Character("y".into()), Modifiers::ALT),
        (Key::Character("y".into()), Modifiers::META),
    ] {
        harness.press_with(key, mods);
        assert!(!matches_focus_visible(&harness, "#target"));
    }

    harness.press(Key::Character("a".into()));
    assert!(matches_focus_visible(&harness, "#target"));

    for (key, mods) in [
        (Key::Named(NamedKey::Control), Modifiers::CONTROL),
        (Key::Named(NamedKey::Alt), Modifiers::ALT),
        (Key::Named(NamedKey::Meta), Modifiers::META),
        (Key::Named(NamedKey::Shift), Modifiers::SHIFT),
    ] {
        harness.press_with(key, mods);
        assert!(matches_focus_visible(&harness, "#target"));
    }
}

#[test]
fn ime_composition_upgrades_pointer_focused_element_to_focus_visible() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <div id="target" tabindex="0" style="width:100px; height:24px;">Target</div>
        </body></html>"#,
    );

    harness.click("#target");
    assert!(!matches_focus_visible(&harness, "#target"));

    harness.ime(BlitzImeEvent::Preedit("ka".into(), Some((0, 2))));
    assert!(matches_focus_visible(&harness, "#target"));

    harness.click("#target");
    assert!(!matches_focus_visible(&harness, "#target"));

    harness.ime(BlitzImeEvent::Commit("ka".into()));
    assert!(matches_focus_visible(&harness, "#target"));
}

#[test]
fn autofocus_and_initial_script_focus_match_focus_visible() {
    let harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <button id="auto" autofocus style="display:block; width:100px; height:24px;">Auto</button>
        </body></html>"#,
    );
    assert_eq!(harness.focused(), Some(harness.node("#auto")));
    assert!(matches_focus_visible(&harness, "#auto"));

    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <div id="target" tabindex="0" style="width:100px; height:24px;">Target</div>
        </body></html>"#,
    );
    let target = harness.node("#target");
    harness.base_mut().set_focus_to(target);
    harness.pump();
    assert!(matches_focus_visible(&harness, "#target"));
}

#[test]
fn dynamic_type_and_contenteditable_mutations_resync_focus_visible() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <input id="inp" type="button" value="Switch" style="display:block; width:100px; height:24px;">
            <div id="editable" tabindex="0" style="width:100px; height:24px;">Box</div>
        </body></html>"#,
    );

    let inp = harness.node("#inp");
    let type_attr = QualName::new(None, ns!(), local_name!("type"));

    harness.click("#inp");
    assert!(!matches_focus_visible(&harness, "#inp"));

    harness
        .base_mut()
        .mutate()
        .set_attribute(inp, type_attr.clone(), "text");
    harness.pump();
    assert!(matches_focus_visible(&harness, "#inp"));

    harness
        .base_mut()
        .mutate()
        .set_attribute(inp, type_attr, "button");
    harness.pump();
    assert!(!matches_focus_visible(&harness, "#inp"));

    let editable = harness.node("#editable");
    let ce_attr = QualName::new(None, ns!(), local_name!("contenteditable"));

    harness.click("#editable");
    assert!(!matches_focus_visible(&harness, "#editable"));

    harness
        .base_mut()
        .mutate()
        .set_attribute(editable, ce_attr.clone(), "true");
    harness.pump();
    assert!(matches_focus_visible(&harness, "#editable"));

    harness
        .base_mut()
        .mutate()
        .set_attribute(editable, ce_attr, "false");
    harness.pump();
    assert!(!matches_focus_visible(&harness, "#editable"));
}

#[test]
fn script_focus_modality_transitions_match_w3c_rules() {
    let make = || {
        Harness::from_html(
            r#"<html><body style="margin:0">
                <div id="unfocusable" style="width:120px; height:20px;">Plain</div>
                <div id="focusable" tabindex="0" style="width:120px; height:20px;">Focusable</div>
                <button id="btn" style="display:block; width:120px; height:20px;">Button</button>
                <input id="input" type="text" value="abc" style="display:block; width:120px; height:20px;">
                <div id="target" tabindex="-1" style="width:120px; height:20px;">Target</div>
            </body></html>"#,
        )
    };

    // Initial script focus with no prior user interaction matches :focus-visible.
    {
        let mut h = make();
        let target = h.node("#target");
        h.base_mut().set_focus_to(target);
        h.pump();
        assert!(matches_focus_visible(&h, "#target"));
    }

    // Clicking an unfocusable element (with or without clear_focus) preserves initial keyboard modality.
    for explicit_blur in [false, true] {
        let mut h = make();
        h.click("#unfocusable");
        if explicit_blur {
            h.base_mut().clear_focus();
        }
        let target = h.node("#target");
        h.base_mut().set_focus_to(target);
        h.pump();
        assert!(matches_focus_visible(&h, "#target"));
    }

    // Keypress with nothing focused followed by unfocusable click or pointermove preserves keyboard modality.
    {
        let mut h = make();
        h.press(Key::Character("a".into()));
        h.click("#unfocusable");
        h.move_mouse_to(10.0, 10.0);
        let target = h.node("#target");
        h.base_mut().set_focus_to(target);
        h.pump();
        assert!(matches_focus_visible(&h, "#target"));
    }

    // Pointer click on focusable div or button (with or without blur) disarms script :focus-visible.
    for click_sel in ["#focusable", "#btn"] {
        for explicit_blur in [false, true] {
            let mut h = make();
            h.click(click_sel);
            if explicit_blur {
                h.base_mut().clear_focus();
            }
            let target = h.node("#target");
            h.base_mut().set_focus_to(target);
            h.pump();
            assert!(!matches_focus_visible(&h, "#target"));
        }
    }

    // Pointer click on text <input> shows :focus-visible on the input itself, but disarms modality for subsequent script focus.
    for explicit_blur in [false, true] {
        let mut h = make();
        h.click("#input");
        assert!(matches_focus_visible(&h, "#input"));
        if explicit_blur {
            h.base_mut().clear_focus();
        }
        let target = h.node("#target");
        h.base_mut().set_focus_to(target);
        h.pump();
        assert!(!matches_focus_visible(&h, "#target"));
    }

    // Pointer click followed by keypress (whether focused or blurred) re-arms modality for script focus.
    for click_sel in ["#focusable", "#input"] {
        for blur_before_key in [false, true] {
            for blur_after_key in [false, true] {
                let mut h = make();
                h.click(click_sel);
                if blur_before_key {
                    h.base_mut().clear_focus();
                }
                h.press(Key::Character("a".into()));
                if blur_after_key {
                    h.base_mut().clear_focus();
                }
                let target = h.node("#target");
                h.base_mut().set_focus_to(target);
                h.pump();
                assert!(matches_focus_visible(&h, "#target"));
            }
        }
    }

    // Tab focus followed by pointer click on button or text input disarms modality for script focus.
    for click_sel in ["#btn", "#input"] {
        let mut h = make();
        h.press(Key::Named(NamedKey::Tab));
        assert!(matches_focus_visible(&h, "#focusable"));
        h.click(click_sel);
        let target = h.node("#target");
        h.base_mut().set_focus_to(target);
        h.pump();
        assert!(!matches_focus_visible(&h, "#target"));
    }
}

#[test]
fn dioxus_click_and_roving_keydown_handlers_preserve_active_modality() {
    #[derive(Default)]
    struct Handles {
        first: RefCell<Option<Rc<MountedData>>>,
        second: RefCell<Option<Rc<MountedData>>>,
        dialog_target: RefCell<Option<Rc<MountedData>>>,
    }

    fn app() -> Element {
        let handles = use_hook(|| Rc::new(Handles::default()));
        let h_first = Rc::clone(&handles);
        let h_second = Rc::clone(&handles);
        let h_dialog = Rc::clone(&handles);
        let h_click = Rc::clone(&handles);
        let h_keydown = Rc::clone(&handles);

        rsx! {
            div { style: "margin: 0;",
                style { ":focus-visible {{ outline: 2px solid rgb(0, 150, 0); }}" }
                button {
                    id: "open-btn",
                    style: "display: block; width: 100px; height: 24px;",
                    onclick: move |_| {
                        let target = h_click.dialog_target.borrow().clone().unwrap();
                        spawn(async move {
                            target.set_focus(true).await.unwrap();
                        });
                    },
                    "Open"
                }
                button {
                    id: "tab-1",
                    style: "display: block; width: 100px; height: 24px;",
                    onmounted: move |e| *h_first.first.borrow_mut() = Some(e.data()),
                    onkeydown: move |evt| {
                        if evt.key() == Key::Named(NamedKey::ArrowRight) {
                            evt.prevent_default();
                            let second = h_keydown.second.borrow().clone().unwrap();
                            spawn(async move {
                                second.set_focus(true).await.unwrap();
                            });
                        }
                    },
                    "Tab 1"
                }
                button {
                    id: "tab-2",
                    style: "display: block; width: 100px; height: 24px;",
                    onmounted: move |e| *h_second.second.borrow_mut() = Some(e.data()),
                    "Tab 2"
                }
                div {
                    id: "dialog-target",
                    tabindex: "-1",
                    style: "width: 100px; height: 24px;",
                    onmounted: move |e| *h_dialog.dialog_target.borrow_mut() = Some(e.data()),
                    "Dialog target"
                }
            }
        }
    }

    let mut harness = Harness::from_component(app);

    // Mouse click on #open-btn focuses #dialog-target inside onclick without :focus-visible.
    harness.click("#open-btn");
    assert_eq!(harness.focused(), Some(harness.node("#dialog-target")));
    assert!(matches_focus(&harness, "#dialog-target"));
    assert!(!matches_focus_visible(&harness, "#dialog-target"));

    // Mouse click on #tab-1 focuses without :focus-visible; ArrowRight roving focuses #tab-2 with :focus-visible.
    harness.click("#tab-1");
    assert_eq!(harness.focused(), Some(harness.node("#tab-1")));
    assert!(!matches_focus_visible(&harness, "#tab-1"));

    harness.press(Key::Named(NamedKey::ArrowRight));
    assert_eq!(harness.focused(), Some(harness.node("#tab-2")));
    assert!(matches_focus_visible(&harness, "#tab-2"));

    // Explicit FocusOptions overrides :focus-visible in place.
    let tab2_id = harness.node("#tab-2");
    harness.base_mut().set_focus_to_with_options(
        tab2_id,
        FocusSource::Script,
        FocusOptions {
            prevent_scroll: true,
            focus_visible: Some(false),
        },
    );
    harness.pump();
    assert!(!matches_focus_visible(&harness, "#tab-2"));
}
