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
    assert_eq!(
        h.base()
            .get_node(s)
            .unwrap()
            .element_data()
            .unwrap()
            .inline_layout_data
            .as_ref()
            .unwrap()
            .text,
        "Charlie"
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
fn select_clipped_wrapper_with_pointer_transparent_arrow() {
    for height in ["2.2em", "auto"] {
        for content in [" ", "\\00a0"] {
            let mut h = Harness::from_html(&format!(
                r#"<style>
        .frm__select {{ display:inline-block; position:relative; overflow:hidden; width:145px; height:auto }}
        .frm__select select {{ appearance:none; background:none; cursor:pointer; margin:0; padding:0 28px 0 .75em; position:relative; display:block; min-width:10em; width:140% !important; height:2.2em !important; border:none !important }}
        .frm__select:after {{ position:absolute; right:10px; top:50%; pointer-events:none; content:'{content}'; width:8px; height:5px; margin-top:-2px; background:black }}
        .frm__select select {{ height:{height} !important }}
        </style><div class="frm__select"><select id="s"><option>All regions</option><option>Other</option></select></div>"#
            ));
            let s = h.node("#s");
            let wrapper = h.layout_rect(".frm__select");
            for (x, y) in [
                (wrapper.x + 20.0, wrapper.y + wrapper.height / 2.0),
                (
                    wrapper.x + wrapper.width - 14.0,
                    wrapper.y + wrapper.height / 2.0 + 2.0,
                ),
            ] {
                click(&mut h, x, y);
                assert_eq!(h.base().open_select, Some(s));
                h.base_mut().close_select();
            }
        }
    }
}

#[test]
fn select_intrinsic_sizing_and_clipped_overflow() {
    let h = Harness::from_html(
        r#"<style>
        select { margin:0 }
        .flex { display:flex; flex-direction:column; width:100px; overflow:scroll }
        .flex select { width:100% }
        </style>
        <div class="flex" id="overflow"><div><select><option>Very very very very long label</option></select></div></div>
        <div style="display:flex;flex-direction:column;height:0"><select id="zero"><option>Label</option></select></div>
        <select id="reference" style="width:100%"><option>Label</option></select>
        <select id="ratio" style="height:50px;aspect-ratio:1"><option>Very very very very long label</option></select>
        <select id="ratio-zero-min" style="height:50px;aspect-ratio:1;min-width:0"><option>Very very very very long label</option></select>
        <select id="contained" style="contain:size"><option>Very very very very long label</option></select>
        <select id="inline-contained" style="contain:inline-size"><option>Very very very very long label</option></select>
        <select id="empty"></select>"#,
    );
    let overflow_right = h
        .base()
        .get_node(h.node("#overflow"))
        .unwrap()
        .final_layout()
        .scrollable_overflow_rect
        .right();
    assert!(overflow_right <= 100.0);
    assert_eq!(
        h.layout_rect("#zero").height,
        h.layout_rect("#reference").height
    );
    assert_eq!(
        h.layout_rect("#zero").width,
        h.layout_rect("#reference").width
    );
    assert_eq!(h.layout_rect("#ratio-zero-min").width, 50.0);
    assert_eq!(h.layout_rect("#ratio-zero-min").height, 50.0);
    assert!(h.layout_rect("#ratio").width > 50.0);
    assert_eq!(h.layout_rect("#ratio").height, 50.0);
    assert_eq!(
        h.layout_rect("#contained").width,
        h.layout_rect("#empty").width
    );
    assert_eq!(
        h.layout_rect("#contained").height,
        h.layout_rect("#empty").height
    );
    assert_eq!(
        h.layout_rect("#inline-contained").width,
        h.layout_rect("#empty").width
    );
}

#[test]
fn select_anonymous_label_uses_select_text_styles() {
    let mut h = Harness::from_html(
        r#"<select id="s" style="text-transform:uppercase"><option label="Alpha  label">Fallback</option><option>Beta</option></select>"#,
    );
    let s = h.node("#s");
    let text = |h: &Harness| {
        h.base()
            .get_node(s)
            .unwrap()
            .element_data()
            .unwrap()
            .inline_layout_data
            .as_ref()
            .unwrap()
            .text
            .clone()
    };
    assert_eq!(text(&h), "ALPHA  LABEL");
    key(&mut h, NamedKey::ArrowDown);
    assert_eq!(text(&h), "ALPHA  LABEL");
    h.base_mut().set_focus_to(s);
    key(&mut h, NamedKey::ArrowDown);
    assert_eq!(text(&h), "BETA");
    assert_eq!(h.text_content("#s"), "FallbackBeta");
}

#[test]
fn select_label_paints_only_inside_content_box() {
    use anyrender::render_to_buffer;
    use anyrender_vello_cpu::VelloCpuImageRenderer;
    use blitz_paint::paint_scene;

    for (options, height) in [
        ("<option>MMMMMMMMMMMMMMMM</option>", 30),
        ("", 30),
        ("<option>MMMMMMMMMMMMMMMM</option>", 0),
    ] {
        let mut h = Harness::from_html_with(
            &format!(
                r#"<body style="margin:20px;background:white"><select id="s" style="appearance:none;overflow:visible;font:20px sans-serif;color:red;border:2px solid blue;background:white;padding:2px 10px;width:60px;height:{height}px">{options}</select></body>"#
            ),
            HarnessOptions {
                width: 160,
                height: 80,
                ..Default::default()
            },
        );
        let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| paint_scene(scene, &mut h.base_mut(), 1.0, 160, 80, 0, 0),
            160,
            80,
        );
        let mut red_pixels = 0;
        for (i, pixel) in buffer.chunks_exact(4).enumerate() {
            if pixel[0] > 160 && pixel[1] < 100 && pixel[2] < 100 {
                red_pixels += 1;
                let (x, y) = (i % 160, i / 160);
                assert!(
                    (32..68).contains(&x) && (24..46).contains(&y),
                    "label pixel outside content box at {x},{y}"
                );
            }
        }
        assert_eq!(red_pixels > 0, !options.is_empty() && height > 0);
    }
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
    let doc = h.base();
    assert_eq!(
        doc.get_node(h.node("#indented"))
            .unwrap()
            .unrounded_layout()
            .size
            .width,
        doc.get_node(h.node("#inline"))
            .unwrap()
            .unrounded_layout()
            .size
            .width
    );
    assert!(indented.height < 32.0);
    assert_eq!(
        h.layout_rect("#empty-indented").height,
        h.layout_rect("#empty").height
    );
    assert_eq!(h.layout_rect("#narrow").height, inline.height);
}
