use blitz_dom::SelectionPoint;
use blitz_test_harness::Harness;
use blitz_traits::node_id::NodeId;

fn text(h: &Harness, selector: &str, offset: usize) -> SelectionPoint {
    SelectionPoint::text(h.node(selector), offset)
}

fn element(h: &Harness, selector: &str, index: usize) -> SelectionPoint {
    SelectionPoint::element(h.node(selector), index)
}

fn selected(h: &mut Harness, start: SelectionPoint, end: SelectionPoint) -> Option<String> {
    h.base_mut().set_text_selection(start, end);
    h.base().get_selected_text()
}

macro_rules! select {
    ($h:ident, $start:expr, $end:expr) => {{
        let start = $start;
        let end = $end;
        selected(&mut $h, start, end)
    }};
}

#[test]
fn text_offsets_are_flattened_layout_utf8_bytes() {
    let mut h = Harness::from_html("<p id='text'>é🙂中</p>");
    let start = text(&h, "#text", 2);
    let end = text(&h, "#text", 6);
    assert_eq!(select!(h, start, end).as_deref(), Some("🙂"));
    assert_eq!(h.base().text_selection().anchor, Some(start));
    assert_eq!(select!(h, text(&h, "#text", 1), end), None);
    assert!(h.base().text_selection().anchor.is_none());
}

#[test]
fn structural_offsets_count_dom_children_even_inside_an_inline_root() {
    let mut h = Harness::from_html(
        "<p id='parent'><span>First</span><span>Second</span><span>Third</span></p>",
    );
    let start = element(&h, "#parent", 1);
    let end = element(&h, "#parent", 2);
    assert_eq!(select!(h, start, end).as_deref(), Some("Second"));
    assert_eq!(select!(h, end, start).as_deref(), Some("Second"));
    assert_eq!(
        select!(h, element(&h, "#parent", 0), element(&h, "#parent", 4)),
        None
    );
}

#[test]
fn mixed_boundaries_order_against_layout_offsets() {
    let mut h =
        Harness::from_html("<p id='parent'><span>First</span><span id='second'>Second</span></p>");
    let offset = text(&h, "#parent", 8);
    assert_eq!(
        select!(h, element(&h, "#parent", 1), offset).as_deref(),
        Some("Sec")
    );
    assert_eq!(
        select!(h, offset, element(&h, "#parent", 2)).as_deref(),
        Some("ond")
    );
}

#[test]
fn collapsed_and_preserved_whitespace_use_layout_coordinates() {
    let mut h = Harness::from_html("<p id='parent'>First   <span>Second</span>   Last</p>");
    assert_eq!(
        select!(h, element(&h, "#parent", 0), element(&h, "#parent", 3)).as_deref(),
        Some("First Second Last"),
    );
    let mut pre = Harness::from_html("<pre id='text'>A  B\nC</pre>");
    assert_eq!(
        select!(pre, text(&pre, "#text", 1), text(&pre, "#text", 5)).as_deref(),
        Some("  B\n"),
    );
}

#[test]
fn text_transform_expansions_are_layout_bytes_not_source_bytes() {
    let mut h = Harness::from_html("<p id='text' style='text-transform:uppercase'>aßé</p>");
    assert_eq!(
        select!(h, text(&h, "#text", 1), text(&h, "#text", 3)).as_deref(),
        Some("SS")
    );
    assert_eq!(
        select!(h, text(&h, "#text", 3), text(&h, "#text", 5)).as_deref(),
        Some("É")
    );
}

#[test]
fn br_projects_between_dom_children() {
    let mut h = Harness::from_html("<p id='parent'>First<br>Second</p>");
    assert_eq!(
        select!(h, element(&h, "#parent", 1), element(&h, "#parent", 2)).as_deref(),
        Some("\n")
    );
    assert_eq!(
        select!(h, element(&h, "#parent", 0), element(&h, "#parent", 3)).as_deref(),
        Some("First\nSecond")
    );
}

#[test]
fn generated_content_is_not_copied_from_element_boundaries() {
    let mut h = Harness::from_html(
        "<style>p::before { content: 'Before'; } p::after { content: 'After'; }</style><p id='parent'>Text</p>",
    );
    assert_eq!(
        select!(h, element(&h, "#parent", 0), element(&h, "#parent", 1)).as_deref(),
        Some("Text")
    );
}

#[test]
fn inline_boxes_copy_in_dom_order() {
    let mut h = Harness::from_html(
        "<p id='parent'>Before<span style='display:inline-block'>Inside</span>After</p>",
    );
    assert_eq!(
        select!(h, element(&h, "#parent", 0), element(&h, "#parent", 3)).as_deref(),
        Some("BeforeInsideAfter")
    );
    assert_eq!(h.base().get_text_selection_ranges().len(), 3);
    assert_eq!(
        select!(h, element(&h, "#parent", 1), element(&h, "#parent", 2)).as_deref(),
        Some("Inside")
    );
}

#[test]
fn bidi_and_grapheme_clusters_copy_in_logical_order() {
    for content in ["אבג DEF דהו", "f<span>i</span>", "a<span>\u{301}</span>"] {
        let mut h = Harness::from_html(&format!("<p id='parent'>{content}</p>"));
        let expected = content.replace("<span>", "").replace("</span>", "");
        assert_eq!(
            select!(
                h,
                element(&h, "#parent", 0),
                element(
                    &h,
                    "#parent",
                    h.base().get_node(h.node("#parent")).unwrap().children.len()
                )
            )
            .as_deref(),
            Some(expected.as_str())
        );
    }
}

#[test]
fn inline_box_edges_with_the_same_layout_offset_remain_distinct() {
    let mut h = Harness::from_html(
        "<p id='parent' style='font:20px/30px monospace'>Before<span id='box' style='display:inline-block'>Inside</span>After</p>",
    );
    let inline_box = h.layout_rect("#box");
    let before_edge = (inline_box.x - 1.0, inline_box.y + 10.0);
    let after_edge = (inline_box.x + inline_box.width + 1.0, inline_box.y + 10.0);
    assert_eq!(
        h.base().find_text_position(before_edge.0, before_edge.1),
        Some(element(&h, "#parent", 1))
    );
    assert_eq!(
        h.base().find_text_position(after_edge.0, after_edge.1),
        Some(element(&h, "#parent", 2))
    );
    h.drag(before_edge, (700.0, 550.0), 3);
    assert_eq!(h.base().get_selected_text().as_deref(), Some("InsideAfter"));
    h.drag(after_edge, (700.0, 550.0), 3);
    assert_eq!(h.base().get_selected_text().as_deref(), Some("After"));
}

#[test]
fn nested_inline_box_edges_are_structural_boundaries() {
    let h = Harness::from_html(
        "<p id='parent' style='font:20px/30px monospace'>Before<span id='wrapper'><span id='box' style='display:inline-block'>Inside</span></span>After</p>",
    );
    let inline_box = h.layout_rect("#box");
    let before_edge = (inline_box.x - 1.0, inline_box.y + 10.0);
    let after_edge = (inline_box.x + inline_box.width + 1.0, inline_box.y + 10.0);
    assert_eq!(
        h.base().find_text_position(before_edge.0, before_edge.1),
        Some(element(&h, "#wrapper", 0))
    );
    assert_eq!(
        h.base().find_text_position(after_edge.0, after_edge.1),
        Some(element(&h, "#wrapper", 1))
    );
}

#[test]
fn display_contents_preserves_child_indices_and_user_select_none() {
    let mut h = Harness::from_html(
        "<p id='parent' style='font:20px/30px monospace'>Before<span id='contents' style='display:contents'>Inside</span>After</p>",
    );
    assert_eq!(
        select!(h, element(&h, "#parent", 1), element(&h, "#parent", 2)).as_deref(),
        Some("Inside")
    );
    let mut h = Harness::from_html(
        "<p id='parent' style='font:20px/30px monospace'><span style='display:contents;user-select:none'>Inside</span>After</p>",
    );
    let rect = h.layout_rect("#parent");
    h.drag((rect.x + 1.0, rect.y + 10.0), (700.0, 550.0), 3);
    assert!(h.base().text_selection().anchor.is_none());
}

#[test]
fn contextual_lowercase_offsets_use_rendered_bytes() {
    let mut h = Harness::from_html("<p id='text' style='text-transform:lowercase'>ΟΣ</p>");
    assert_eq!(
        select!(h, text(&h, "#text", 2), text(&h, "#text", 4)).as_deref(),
        Some("ς")
    );
}

#[test]
fn user_select_none_is_excluded() {
    let mut h =
        Harness::from_html("<p id='parent'>A<span style='user-select:none'>Blocked</span>B</p>");
    assert_eq!(
        select!(h, element(&h, "#parent", 0), element(&h, "#parent", 3)).as_deref(),
        Some("AB")
    );
    assert_eq!(h.base().get_text_selection_ranges().len(), 2);
}

#[test]
fn anonymous_text_endpoint_survives_layout_reconstruction() {
    let mut h = Harness::from_html("<div id='parent'>First<div>Middle</div>Last</div>");
    let parent = h.node("#parent");
    let first: NodeId = h.base().get_node(parent).unwrap().children[0];
    let root = h
        .base()
        .get_node(first)
        .unwrap()
        .inline_root_ancestor()
        .unwrap()
        .id;
    assert_eq!(
        select!(h, SelectionPoint::text(root, 2), element(&h, "#parent", 3)).as_deref(),
        Some("rst Middle Last")
    );
    let anchor = h.base().text_selection().anchor;
    h.base_mut().set_style_property(parent, "padding", "10px");
    h.pump();
    assert_eq!(h.base().text_selection().anchor, anchor);
    assert_eq!(
        h.base().get_selected_text().as_deref(),
        Some("rst Middle Last")
    );
}

#[test]
fn pointer_inside_text_anchors_to_the_inline_root() {
    let mut h = Harness::from_html(
        "<p id='parent' style='font:20px/30px monospace'><span id='text'>Text</span></p>",
    );
    let rect = h.layout_rect("#text");
    h.drag((rect.x + 1.0, rect.y + 10.0), (700.0, 550.0), 3);
    assert_eq!(
        h.base().text_selection().anchor,
        Some(text(&h, "#parent", 0))
    );
    assert_eq!(h.base().get_selected_text().as_deref(), Some("Text"));
}

#[test]
fn empty_element_is_a_structural_anchor() {
    let mut h = Harness::from_html("<div id='empty' style='height:40px'></div><p>Text</p>");
    h.drag(h.center_of("#empty"), (700.0, 550.0), 3);
    assert_eq!(
        h.base().text_selection().anchor,
        Some(element(&h, "#empty", 0))
    );
    assert_eq!(h.base().get_selected_text().as_deref(), Some("Text"));
}

#[test]
fn reflow_preserves_layout_offsets() {
    let mut h = Harness::from_html(
        "<p id='text' style='width:300px; font:20px/30px monospace'>First second third</p>",
    );
    let container = h.node("#text");
    assert_eq!(
        select!(h, text(&h, "#text", 6), text(&h, "#text", 12)).as_deref(),
        Some("second")
    );
    h.base_mut().set_style_property(container, "width", "50px");
    h.pump();
    assert_eq!(h.base().get_selected_text().as_deref(), Some("second"));
}
