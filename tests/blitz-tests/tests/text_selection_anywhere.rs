use blitz_test_harness::{Harness, HarnessOptions, pointer_event};
use blitz_traits::events::{BlitzPointerId, MouseEventButton, MouseEventButtons, UiEvent};

const HTML: &str = r#"<!doctype html><html><head><style>
    body { margin: 0; font: 20px/30px monospace; }
    #container { margin: 40px; width: 240px; padding: 20px; border: 5px solid; }
    p { margin: 0; }
    #gap { height: 40px; }
</style></head><body><div id="container"><p id="first">First paragraph</p><div id="gap"></div><p id="second">Second paragraph</p></div></body></html>"#;

fn assert_position(h: &Harness, point: (f32, f32), selector: &str, offset: usize) {
    assert_eq!(
        h.base().find_text_position(point.0, point.1),
        Some((h.node(selector), offset)),
        "position at {point:?}"
    );
}

#[test]
fn boundaries_in_padding_margins_empty_boxes_and_page_background() {
    let h = Harness::from_html(HTML);
    let first = h.layout_rect("#first");
    let second = h.layout_rect("#second");
    let gap = h.layout_rect("#gap");
    assert!(first.height > 0.0, "requires a usable font");
    assert_position(&h, (first.x, first.y - 10.0), "#container", 0);
    assert_position(&h, (first.x - 50.0, first.y + 15.0), "body", 0);
    assert_position(&h, (gap.x + 1.0, gap.y + 5.0), "#gap", 0);
    assert_position(
        &h,
        (second.x, second.y + second.height + 10.0),
        "#container",
        3,
    );
    assert_position(&h, (700.0, 550.0), "html", 2);
}

#[test]
fn drag_from_block_padding_selects_across_paragraphs() {
    let mut h = Harness::from_html(HTML);
    let first = h.layout_rect("#first");
    let second = h.layout_rect("#second");
    h.drag(
        (first.x, first.y - 10.0),
        (second.x + second.width - 1.0, second.y + 15.0),
        4,
    );
    assert_eq!(
        h.base().get_selected_text().as_deref(),
        Some("First paragraph Second paragraph")
    );
}

#[test]
fn drag_from_page_background_back_to_first_paragraph() {
    let mut h = Harness::from_html(HTML);
    let first = h.layout_rect("#first");
    h.drag((700.0, 550.0), (first.x - 50.0, first.y + 15.0), 4);
    assert_eq!(
        h.base().get_selected_text().as_deref(),
        Some("First paragraph Second paragraph")
    );
}

#[test]
fn drag_continues_outside_document_boxes() {
    let mut h = Harness::from_html(HTML);
    let first = h.layout_rect("#first");
    h.drag((first.x, first.y + 15.0), (900.0, 700.0), 4);
    assert_eq!(
        h.base().get_selected_text().as_deref(),
        Some("First paragraph Second paragraph")
    );
}

#[test]
fn start_and_extend_around_anonymous_inline_roots() {
    let mut h = Harness::from_html(
        r#"<html><body style="margin:40px; font:20px/30px monospace">Outer<div>Inner</div></body></html>"#,
    );
    h.drag((10.0, 45.0), (700.0, 550.0), 4);
    assert_eq!(h.base().get_selected_text().as_deref(), Some("Outer Inner"));
}

#[test]
fn padding_stays_in_the_clicked_container() {
    let h = Harness::from_html(
        r#"<html><body style="margin:0; font:20px/30px monospace">
        <div id="container" style="padding-bottom:100px"><p id="first" style="margin:0">First</p></div>
        <p style="margin:0">Second</p>
    </body></html>"#,
    );
    assert_position(&h, (5.0, 120.0), "#container", 1);
}

#[test]
fn column_padding_is_a_child_boundary() {
    let h = Harness::from_html(
        r#"<html><body style="margin:0; font:20px/30px monospace">
        <div id="columns" style="display:flex; gap:40px; padding:40px">
            <div id="left" style="width:200px">Left</div><div id="right" style="width:200px">Right</div>
        </div>
    </body></html>"#,
    );
    assert_position(&h, (265.0, 20.0), "#columns", 1);
}

#[test]
fn user_select_none_on_ancestors_prevents_starting_a_drag() {
    let mut h = Harness::from_html(
        r#"<html><body style="margin:0; font:20px/30px monospace">
        <div style="user-select:none"><div><div id="blocked" style="padding:20px">Blocked</div></div></div>
        <div id="allowed">Allowed</div>
    </body></html>"#,
    );
    h.drag((5.0, 5.0), h.center_of("#allowed"), 3);
    assert!(!h.base().has_text_selection());
}

#[test]
fn explicit_user_select_text_overrides_none() {
    let mut h = Harness::from_html(
        r#"<html><body style="margin:0; font:20px/30px monospace; user-select:none">
        <div id="allowed" style="user-select:text; padding:20px">Allowed</div>
    </body></html>"#,
    );
    h.drag((5.0, 5.0), (250.0, 35.0), 3);
    assert_eq!(h.base().get_selected_text().as_deref(), Some("Allowed"));
}

#[test]
fn controls_do_not_start_a_document_selection() {
    for control in [
        r#"<input type="text" value="Input">"#,
        r#"<input type="checkbox">"#,
        "<button>Button</button>",
    ] {
        let mut h = Harness::from_html(&format!(
            r#"<html><body style="margin:0; font:20px/30px monospace"><div id="control">{control}</div><div id="text">Text</div></body></html>"#
        ));
        h.drag(h.center_of("#control > *"), h.center_of("#text"), 3);
        assert!(!h.base().has_text_selection());
    }
}

#[test]
fn empty_document_can_have_an_element_selection() {
    let mut h = Harness::from_html("<html><body><div style='height:100px'></div></body></html>");
    h.drag((10.0, 10.0), (100.0, 200.0), 3);
    assert!(h.base().has_text_selection());
    assert_eq!(h.base().get_selected_text(), None);
}

#[test]
fn non_primary_mouse_button_does_not_start_selection() {
    let mut h = Harness::from_html(HTML);
    let event = pointer_event(
        BlitzPointerId::Mouse,
        50.0,
        50.0,
        MouseEventButton::Secondary,
        MouseEventButtons::from(MouseEventButton::Secondary),
        Default::default(),
    );
    h.dispatch(UiEvent::PointerDown(event));
    h.dispatch(UiEvent::PointerMove(pointer_event(
        BlitzPointerId::Mouse,
        700.0,
        550.0,
        MouseEventButton::Secondary,
        MouseEventButtons::from(MouseEventButton::Secondary),
        Default::default(),
    )));
    assert!(!h.base().has_text_selection());
}

#[test]
fn boundaries_respect_transforms_and_viewport_scale() {
    for scale in [1.0, 2.0] {
        let h = Harness::from_html_with(
            r#"<html><body style="margin:0; font:20px/30px monospace">
            <div id="transformed" style="transform:translate(100px, 70px); padding:20px"><p id="text" style="margin:0">Text</p></div>
        </body></html>"#,
            HarnessOptions {
                scale,
                ..Default::default()
            },
        );
        assert_position(&h, (110.0, 75.0), "#transformed", 0);
    }
}

#[test]
fn boundaries_respect_scrolled_containers() {
    let mut h = Harness::from_html(
        r#"<html><body style="margin:0; font:20px/30px monospace">
        <div id="scroller" style="height:100px; width:300px; overflow:auto; padding:20px">
            <div style="height:200px"></div><p id="text" style="margin:0">Text</p><div style="height:200px"></div>
        </div>
    </body></html>"#,
    );
    h.wheel_at(50.0, 50.0, 0.0, -200.0);
    assert_position(&h, (10.0, 10.0), "#scroller", 1);
}

#[test]
fn whitespace_does_not_snap_to_out_of_flow_text() {
    for position in ["absolute", "fixed"] {
        let h = Harness::from_html(&format!(
            r#"<html><body style="margin:0; font:20px/30px monospace">
            <div id="container" style="position:relative; height:200px">
                <div><p id="text" style="position:{position}; left:100px; top:80px; margin:0">Text</p></div>
            </div>
        </body></html>"#
        ));
        assert_position(&h, (100.0, 70.0), "#container", 2);
    }
}

#[test]
fn empty_inline_block_child_is_a_boundary_container() {
    let h = Harness::from_html(
        r#"<html><body style="margin:0; font:20px/30px monospace">
        <p style="margin:0">Before
            <span style="display:inline-block; margin-left:30px; padding:10px">
                <span id="empty" style="display:block; height:20px"></span>
                <span id="text" style="display:block">Inside</span>
            </span>
        </p>
    </body></html>"#,
    );
    let empty = h.layout_rect("#empty");
    assert_position(&h, (empty.x + 1.0, empty.y + 5.0), "#empty", 0);
}

#[test]
fn background_before_hidden_content_is_a_root_boundary() {
    for css in ["visibility:hidden", "display:none", "transform:scale(0)"] {
        let h = Harness::from_html(&format!(
            r#"<html><body style="margin:0; font:20px/30px monospace">
            <div style="{css}">Hidden</div><p id="text" style="margin:0">Visible</p>
        </body></html>"#
        ));
        assert_position(&h, (0.0, -10.0), "html", 0);
    }
}

#[test]
fn visible_descendant_of_hidden_parent_can_be_selected() {
    let h = Harness::from_html(
        r#"<html><body style="margin:0; font:20px/30px monospace">
        <div style="visibility:hidden; padding:20px"><p id="text" style="visibility:visible; margin:0">Visible</p></div>
    </body></html>"#,
    );
    let text = h.base().get_node(h.node("#text")).unwrap().children[0];
    let (x, y) = h.center_of("#text");
    assert_eq!(
        h.base().find_text_position(x, y),
        Some((text, "Visible".len()))
    );
}

#[test]
fn drag_from_whitespace_preserves_multibyte_text_offsets() {
    let mut h = Harness::from_html(
        r#"<html><body style="margin:0; font:20px/30px monospace"><div style="padding:20px">é🙂中</div></body></html>"#,
    );
    h.drag((0.0, 0.0), (700.0, 550.0), 3);
    assert_eq!(h.base().get_selected_text().as_deref(), Some("é🙂中"));
}

#[test]
fn document_drag_over_a_text_input_does_not_select_its_value() {
    let mut h = Harness::from_html(
        r#"<html><body style="margin:0; font:20px/30px monospace">
        <div id="text" style="padding:20px">Text</div><input id="input" type="text" value="Input">
    </body></html>"#,
    );
    h.drag((0.0, 0.0), h.center_of("#input"), 3);
    assert_eq!(h.base().get_selected_text().as_deref(), Some("Text"));
}
