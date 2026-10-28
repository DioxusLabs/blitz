use blitz_test_harness::{Harness, HarnessOptions, key_event, pointer_event};
use blitz_traits::events::{
    BlitzPointerId, KeyState, MouseEventButton, MouseEventButtons, UiEvent,
};
use keyboard_types::{Key, Modifiers, NamedKey};

fn fixture() -> Harness {
    Harness::from_html_with(
        r#"<html><body style="margin:0">
        <div style="position:absolute;top:150px;left:20px;height:40px;overflow:hidden;width:280px">
        <select id="s" style="font-size:16px;width:180px"><option id="a">Alpha</option>
        <option disabled id="b">Bravo</option><optgroup disabled label="Unavailable"><option>Blocked</option></optgroup>
        <option id="c" value="charlie">Charlie</option><option id="d">Delta</option></select></div>
        <select id="disabled" disabled style="position:absolute;left:310px"><option>No</option></select>
        <button id="under" style="position:absolute;top:200px">Under</button>
        </body></html>"#,
        HarnessOptions {
            width: 400,
            height: 240,
            ..Default::default()
        },
    )
}

fn pointer(x: f32, y: f32) -> blitz_traits::events::BlitzPointerEvent {
    pointer_event(
        BlitzPointerId::Mouse,
        x,
        y,
        MouseEventButton::Main,
        MouseEventButtons::from(MouseEventButton::Main),
        Modifiers::default(),
    )
}

fn click(h: &mut Harness, x: f32, y: f32) -> Vec<String> {
    let p = pointer(x, y);
    let events = h.dispatch_recorded([UiEvent::PointerDown(p.clone()), UiEvent::PointerUp(p)]);
    h.pump();
    events
}

fn key(h: &mut Harness, key: NamedKey) -> Vec<String> {
    let events = h.dispatch_recorded([UiEvent::KeyDown(key_event(
        Key::Named(key),
        KeyState::Pressed,
        Modifiers::default(),
    ))]);
    h.pump();
    events
}

fn open(h: &mut Harness) {
    let (x, y) = h.layout_rect("#s").center();
    click(h, x, y);
    assert_eq!(h.base().open_select, Some(h.node("#s")));
}

#[test]
fn select_pointer_overlay_selection_and_events() {
    let mut h = fixture();
    open(&mut h);
    let s = h.node("#s");
    let c = h.node("#c");
    let overlay = h.base_mut().select_overlay().unwrap();
    assert!(overlay.rect.y < h.layout_rect("#s").y as f64, "flips above");
    let row = overlay.rows.iter().find(|r| r.option == c).unwrap();
    let events = click(&mut h, (row.rect.x + 5.0) as f32, (row.rect.y + 5.0) as f32);
    assert_eq!(events, ["input", "change"]);
    assert_eq!(h.base().selected_option(s), Some(c));
    assert_eq!(h.base().open_select, None);
    assert!(
        h.base()
            .get_node(s)
            .unwrap()
            .text_content()
            .ends_with("Charlie")
    );
    open(&mut h);
    let row = h
        .base_mut()
        .select_overlay()
        .unwrap()
        .rows
        .into_iter()
        .find(|r| r.option == c)
        .unwrap();
    assert!(
        click(&mut h, (row.rect.x + 5.0) as f32, (row.rect.y + 5.0) as f32).is_empty(),
        "unchanged selection emits nothing"
    );
}

#[test]
fn select_keyboard_commit_cancel_and_skip_disabled() {
    let mut h = fixture();
    let s = h.node("#s");
    let a = h.node("#a");
    let c = h.node("#c");
    open(&mut h);
    key(&mut h, NamedKey::ArrowDown);
    assert_eq!(h.base().selected_option(s), Some(a));
    key(&mut h, NamedKey::Escape);
    assert_eq!(h.base().open_select, None);
    assert_eq!(
        key(&mut h, NamedKey::ArrowDown)
            .iter()
            .filter(|e| e.as_str() == "input" || e.as_str() == "change")
            .cloned()
            .collect::<Vec<_>>(),
        ["input", "change"]
    );
    assert_eq!(h.base().selected_option(s), Some(c));
    key(&mut h, NamedKey::Enter);
    key(&mut h, NamedKey::Home);
    key(&mut h, NamedKey::Enter);
    assert_eq!(h.base().selected_option(s), Some(a));
    assert_eq!(h.base().open_select, None);
    key(&mut h, NamedKey::Enter);
    key(&mut h, NamedKey::End);
    key(&mut h, NamedKey::Tab);
    assert_eq!(h.base().selected_option(s), Some(a));
    assert_eq!(h.base().open_select, None);
}

#[test]
fn select_outside_disabled_blur_and_removal() {
    let mut h = fixture();
    open(&mut h);
    assert!(
        click(&mut h, 30.0, 210.0).is_empty(),
        "outside click consumed"
    );
    assert_eq!(h.base().open_select, None);
    let (x, y) = h.layout_rect("#disabled").center();
    click(&mut h, x, y);
    assert_eq!(h.base().open_select, None);
    open(&mut h);
    let button = h.node("#under");
    h.base_mut().set_focus_to(button);
    assert_eq!(h.base().open_select, None);
    open(&mut h);
    let s = h.node("#s");
    h.base_mut()
        .mutate()
        .set_attribute(s, blitz_dom::qual_name!("disabled"), "");
    assert_eq!(h.base().open_select, None);
    h.base_mut()
        .mutate()
        .clear_attribute(s, blitz_dom::qual_name!("disabled"));
    open(&mut h);
    h.base_mut().mutate().remove_and_drop_node(s);
    assert_eq!(h.base().open_select, None);
}

#[test]
fn select_empty_and_scrolling_overlay() {
    let options = (0..30)
        .map(|i| format!("<option>Option {i}</option>"))
        .collect::<String>();
    let mut h = Harness::from_html_with(
        &format!("<select id='s'>{options}</select><select id='empty'></select>"),
        HarnessOptions {
            height: 1200,
            ..Default::default()
        },
    );
    let s = h.node("#s");
    assert!(h.base_mut().open_select(s));
    let overlay = h.base_mut().select_overlay().unwrap();
    assert_eq!(overlay.rows.len(), 20);
    let first = overlay.rows[0].option;
    h.wheel_at(
        (overlay.rect.x + 5.0) as f32,
        (overlay.rect.y + 5.0) as f32,
        0.0,
        -240.0,
    );
    assert_ne!(h.base_mut().select_overlay().unwrap().rows[0].option, first);
    key(&mut h, NamedKey::End);
    let last = *h.base().select_options(s).last().unwrap();
    assert_eq!(
        h.base_mut()
            .select_overlay()
            .unwrap()
            .rows
            .last()
            .unwrap()
            .option,
        last
    );
    let empty = h.node("#empty");
    assert!(h.base_mut().open_select(empty));
    assert!(h.base_mut().select_overlay().unwrap().rows.is_empty());
    key(&mut h, NamedKey::ArrowDown);
    key(&mut h, NamedKey::Enter);
    assert_eq!(h.base().selected_option(empty), None);
}

#[test]
fn select_overlay_tracks_transforms_scroll_and_scale() {
    let mut h = Harness::from_html_with(
        r#"<body style="margin:0"><div id="scroller" style="position:absolute;left:100px;top:80px;width:280px;height:150px;overflow:auto;transform:translate(20px,10px)"><div style="height:60px"></div><select id="s" style="display:block;width:200px"><option>Alpha</option><option>Beta</option></select><div style="height:300px"></div></div>"#,
        HarnessOptions {
            width: 800,
            height: 800,
            scale: 2.0,
            ..Default::default()
        },
    );
    h.wheel_at(130.0, 100.0, 0.0, -30.0);
    let s = h.node("#s");
    let scroller = h.node("#scroller");
    assert!(h.base().get_node(scroller).unwrap().scroll_offset().y > 0.0);
    let anchor = h.layout_rect("#s");
    assert!(h.base_mut().open_select(s));
    let overlay = h.base_mut().select_overlay().unwrap();
    assert!((overlay.rect.x - anchor.x as f64 - 20.0).abs() < 0.1);
    assert!((overlay.rect.y - anchor.y as f64 - anchor.height as f64 - 10.0).abs() < 0.1);
    let second = overlay.rows[1].option;
    let row = &overlay.rows[1].rect;
    click(&mut h, (row.x + 5.0) as f32, (row.y + 5.0) as f32);
    assert_eq!(h.base().selected_option(s), Some(second));
}

#[test]
fn select_accessibility_expanded_follows_open() {
    let mut h = fixture();
    let s = h.node("#s");
    let expanded = |h: &Harness| {
        h.base()
            .build_accessibility_tree()
            .nodes
            .into_iter()
            .find(|(id, _)| id.0 == s.as_u64())
            .unwrap()
            .1
            .is_expanded()
    };
    assert_eq!(expanded(&h), Some(false));
    open(&mut h);
    assert_eq!(expanded(&h), Some(true));
    key(&mut h, NamedKey::Escape);
    assert_eq!(expanded(&h), Some(false));
}

#[test]
fn select_indented_markup_has_single_line_height() {
    let h = Harness::from_html(
        r#"<body style="font:16px sans-serif">
        <select id="inline"><option>Alpha</option><option selected>Second label</option></select>
        <select id="indented">
            <option>Alpha</option>
            <option selected>Second label</option>
        </select>
        <select id="empty"></select>
        <select id="empty-indented">
        </select>
        <select id="narrow" style="width:50px">
            <option label="A  long label that must not wrap">Fallback</option>
        </select>
    </body>"#,
    );
    let inline = h.layout_rect("#inline");
    let indented = h.layout_rect("#indented");
    assert_eq!(indented.height, inline.height);
    assert_eq!(indented.width, inline.width);
    assert!(indented.height < 32.0);
    assert_eq!(
        h.layout_rect("#empty-indented").height,
        h.layout_rect("#empty").height
    );
    assert_eq!(h.layout_rect("#narrow").height, inline.height);
}
