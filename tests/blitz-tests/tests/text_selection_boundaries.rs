use blitz_dom::SelectionPoint;
use blitz_test_harness::Harness;
use blitz_traits::node_id::NodeId;

fn text_node(h: &Harness, selector: &str) -> NodeId {
    h.base().get_node(h.node(selector)).unwrap().children[0]
}

fn select(h: &mut Harness, a: NodeId, start: usize, b: NodeId, end: usize) -> Option<String> {
    h.base_mut().set_text_selection(a, start, b, end);
    h.base().get_selected_text()
}

#[test]
fn text_offsets_are_source_utf8_bytes() {
    let mut h = Harness::from_html("<p id='text'>é🙂中</p>");
    let id = text_node(&h, "#text");
    assert_eq!(select(&mut h, id, 2, id, 6).as_deref(), Some("🙂"));
    assert_eq!(
        h.base().text_selection().anchor,
        Some(SelectionPoint {
            node: id,
            offset: 2
        })
    );
    assert_eq!(select(&mut h, id, 1, id, 6), None);
    assert!(h.base().text_selection().anchor.is_none());
    assert_eq!(select(&mut h, id, 0, id, 100), None);
}

#[test]
fn element_offsets_count_dom_children() {
    let mut h = Harness::from_html(
        "<div id='parent'><span>First</span><span>Second</span><span>Third</span></div>",
    );
    let parent = h.node("#parent");
    assert_eq!(
        select(&mut h, parent, 1, parent, 2).as_deref(),
        Some("Second")
    );
    assert_eq!(
        select(&mut h, parent, 2, parent, 1).as_deref(),
        Some("Second")
    );
    assert_eq!(select(&mut h, parent, 0, parent, 4), None);
}

#[test]
fn ancestor_boundaries_order_against_descendant_offsets() {
    let mut h = Harness::from_html(
        "<div id='parent'><span>First</span><span id='second'>Second</span></div>",
    );
    let parent = h.node("#parent");
    let second = text_node(&h, "#second");
    assert_eq!(select(&mut h, parent, 1, second, 3).as_deref(), Some("Sec"));
    assert_eq!(select(&mut h, second, 3, parent, 2).as_deref(), Some("ond"));
    assert_eq!(select(&mut h, second, 3, parent, 1).as_deref(), Some("Sec"));
}

#[test]
fn collapsed_whitespace_and_nested_spans_map_back_to_source() {
    let mut h = Harness::from_html(
        "<div id='parent'>  First   <span id='second'>  Second  </span> Last </div>",
    );
    let second = text_node(&h, "#second");
    assert_eq!(
        select(&mut h, second, 2, second, 8).as_deref(),
        Some("Second")
    );
    let parent = h.node("#parent");
    assert_eq!(
        select(&mut h, parent, 0, parent, 3).as_deref(),
        Some("First Second Last")
    );
}

#[test]
fn preserving_whitespace_preserves_source_offsets() {
    let mut h = Harness::from_html("<pre id='text'>A  B\nC</pre>");
    let id = text_node(&h, "#text");
    assert_eq!(select(&mut h, id, 1, id, 5).as_deref(), Some("  B\n"));
}

#[test]
fn text_transform_expansions_use_source_offsets() {
    let mut h = Harness::from_html("<p id='text' style='text-transform:uppercase'>aßé</p>");
    let id = text_node(&h, "#text");
    assert_eq!(select(&mut h, id, 1, id, 3).as_deref(), Some("SS"));
    assert_eq!(select(&mut h, id, 3, id, 5).as_deref(), Some("É"));
}

#[test]
fn contextual_lowercase_uses_source_offsets() {
    let mut h = Harness::from_html("<p id='text' style='text-transform:lowercase'>ΟΣ</p>");
    let id = text_node(&h, "#text");
    assert_eq!(select(&mut h, id, 2, id, 4).as_deref(), Some("ς"));
}

#[test]
fn line_breaks_project_between_element_children() {
    let mut h = Harness::from_html("<p id='parent'>First<br>Second</p>");
    let id = h.node("#parent");
    assert_eq!(
        select(&mut h, id, 0, id, 3).as_deref(),
        Some("First\nSecond")
    );
    assert_eq!(select(&mut h, id, 1, id, 2).as_deref(), Some("\n"));
}

#[test]
fn generated_content_does_not_become_a_dom_endpoint() {
    let mut h = Harness::from_html(
        "<style>p::before { content: 'Before'; } p::after { content: 'After'; }</style><p id='parent'>Text</p>",
    );
    let id = h.node("#parent");
    assert_eq!(select(&mut h, id, 0, id, 1).as_deref(), Some("Text"));
}

#[test]
fn inline_boxes_copy_in_dom_order() {
    let mut h = Harness::from_html(
        "<p id='parent'>Before<span style='display:inline-block'>Inside</span>After</p>",
    );
    let id = h.node("#parent");
    assert_eq!(
        select(&mut h, id, 0, id, 3).as_deref(),
        Some("BeforeInsideAfter")
    );
    assert_eq!(h.base().get_text_selection_ranges().len(), 3);
}

#[test]
fn bidi_text_copies_in_logical_order() {
    let mut h = Harness::from_html("<p id='parent' dir='rtl'>אבג DEF דהו</p>");
    let id = h.node("#parent");
    assert_eq!(select(&mut h, id, 0, id, 1).as_deref(), Some("אבג DEF דהו"));
}

#[test]
fn styled_graphemes_and_ligatures_retain_all_source_text() {
    for text in ["f<span>i</span>", "a<span>\u{301}</span>"] {
        let mut h = Harness::from_html(&format!(
            "<p id='parent' style='font-family:serif'>{text}</p>"
        ));
        let id = h.node("#parent");
        let expected = if text.starts_with('f') {
            "fi"
        } else {
            "a\u{301}"
        };
        assert_eq!(select(&mut h, id, 0, id, 2).as_deref(), Some(expected));
    }
}

#[test]
fn nonselectable_spans_are_excluded_from_projected_ranges() {
    let mut h =
        Harness::from_html("<p id='parent'>A<span style='user-select:none'>Blocked</span>B</p>");
    let id = h.node("#parent");
    assert_eq!(select(&mut h, id, 0, id, 3).as_deref(), Some("AB"));
    assert_eq!(h.base().get_text_selection_ranges().len(), 2);
}

#[test]
fn anonymous_layout_reconstruction_does_not_change_endpoints() {
    let mut h = Harness::from_html("<div id='parent'>First<div>Middle</div>Last</div>");
    let id = h.node("#parent");
    let text = text_node(&h, "#parent");
    assert_eq!(
        select(&mut h, text, 2, id, 3).as_deref(),
        Some("rst Middle Last")
    );
    let anchor = h.base().text_selection().anchor;
    h.base_mut().set_style_property(id, "padding", "10px");
    h.pump();
    assert_eq!(h.base().text_selection().anchor, anchor);
    assert_eq!(
        h.base().get_selected_text().as_deref(),
        Some("rst Middle Last")
    );
}

#[test]
fn pointer_selection_anchors_to_the_original_text_node() {
    let mut h =
        Harness::from_html("<p style='font:20px/30px monospace'><span id='text'>Text</span></p>");
    let id = text_node(&h, "#text");
    let rect = h.layout_rect("#text");
    h.drag((rect.x + 1.0, rect.y + 10.0), (700.0, 550.0), 3);
    let point = h.base().text_selection().anchor.unwrap();
    assert_eq!(point.node, id);
    assert_eq!(point.offset, 0);
    assert_eq!(h.base().get_selected_text().as_deref(), Some("Text"));
}

#[test]
fn empty_element_keeps_its_own_selection_anchor() {
    let mut h = Harness::from_html("<div id='empty' style='height:40px'></div><p>Text</p>");
    let empty = h.node("#empty");
    h.drag(h.center_of("#empty"), (700.0, 550.0), 3);
    assert_eq!(
        h.base().text_selection().anchor,
        Some(SelectionPoint {
            node: empty,
            offset: 0
        })
    );
    assert_eq!(h.base().get_selected_text().as_deref(), Some("Text"));
}

#[test]
fn reflow_preserves_mapping_and_source_offsets() {
    let mut h = Harness::from_html(
        "<p id='text' style='width:300px; font:20px/30px monospace'>First second third</p>",
    );
    let text = text_node(&h, "#text");
    let container = h.node("#text");
    assert_eq!(select(&mut h, text, 6, text, 12).as_deref(), Some("second"));
    h.base_mut().set_style_property(container, "width", "50px");
    h.pump();
    assert_eq!(h.base().get_selected_text().as_deref(), Some("second"));
}

#[test]
fn generated_content_clicks_resolve_to_the_owning_element() {
    let mut h = Harness::from_html(
        "<style>p { font:20px/30px monospace; } p::before { content:'Before'; }</style><p id='text'>Text</p>",
    );
    let id = h.node("#text");
    let rect = h.layout_rect("#text");
    h.drag((rect.x + 1.0, rect.y + 10.0), (700.0, 550.0), 3);
    assert_eq!(
        h.base().text_selection().anchor,
        Some(SelectionPoint {
            node: id,
            offset: 0
        })
    );
    assert_eq!(h.base().get_selected_text().as_deref(), Some("Text"));
}

#[test]
fn display_contents_preserves_dom_text_boundaries() {
    let mut h = Harness::from_html(
        "<p id='parent'>Before<span id='contents' style='display:contents'>Inside</span>After</p>",
    );
    let text = text_node(&h, "#contents");
    assert_eq!(select(&mut h, text, 2, text, 4).as_deref(), Some("si"));
    let parent = h.node("#parent");
    assert_eq!(
        select(&mut h, parent, 0, parent, 3).as_deref(),
        Some("BeforeInsideAfter")
    );
}

#[test]
fn display_contents_user_select_none_cannot_start_a_drag() {
    let mut h = Harness::from_html(
        "<p id='parent' style='font:20px/30px monospace'><span style='display:contents;user-select:none'>Inside</span>After</p>",
    );
    let rect = h.layout_rect("#parent");
    h.drag((rect.x + 1.0, rect.y + 10.0), (700.0, 550.0), 3);
    assert!(h.base().text_selection().anchor.is_none());
}
