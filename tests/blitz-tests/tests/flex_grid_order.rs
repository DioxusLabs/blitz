//! CSS `order` on flex/grid items: a container's `layout_children` is kept
//! persistently sorted in order-modified document order. It is sorted when
//! the container's children are constructed and re-sorted (without
//! reconstructing any boxes) when a child's `order` changes.

use blitz_dom::{DocumentConfig, QualName, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn attr(name: &str) -> QualName {
    QualName::new(None, ns!(), name.into())
}

fn make_doc(html: &str, incremental: bool) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 400, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.set_incremental_layout(incremental);
    doc.resolve(0.0);
    doc
}

fn id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().unwrap()
}

fn x(doc: &HtmlDocument, selector: &str) -> f32 {
    doc.get_node(id(doc, selector))
        .unwrap()
        .final_layout()
        .location
        .x
}

fn y(doc: &HtmlDocument, selector: &str) -> f32 {
    doc.get_node(id(doc, selector))
        .unwrap()
        .final_layout()
        .location
        .y
}

fn layout_children(doc: &HtmlDocument, selector: &str) -> Vec<NodeId> {
    doc.get_node(id(doc, selector))
        .unwrap()
        .layout_children
        .borrow()
        .as_ref()
        .expect("container should have layout children")
        .to_vec()
}

/// Element ids of the given nodes (anonymous boxes are reported as "<anon>").
fn ids_of(doc: &HtmlDocument, nodes: &[NodeId]) -> Vec<String> {
    nodes
        .iter()
        .map(|node_id| {
            let node = doc.get_node(*node_id).unwrap();
            node.element_data()
                .and_then(|el| el.id.as_ref().map(|id| id.to_string()))
                .unwrap_or_else(|| "<anon>".to_string())
        })
        .collect()
}

const FLEX_HTML: &str = r#"<html><body style="margin:0">
    <div id="flex" style="display:flex; width:400px;">
        <div id="a" style="width:10px; height:10px; order: 1"></div>
        <div id="b" style="width:10px; height:10px;"></div>
        <div id="c" style="width:10px; height:10px; order: -1"></div>
        <div id="d" style="width:10px; height:10px; order: 1"></div>
        <div id="e" style="width:10px; height:10px;"></div>
    </div>
</body></html>"#;

#[test]
fn flex_items_are_laid_out_in_order_modified_document_order() {
    for incremental in [false, true] {
        let doc = make_doc(FLEX_HTML, incremental);

        // Expected visual order: c (-1), b (0), e (0), a (1), d (1)
        assert_eq!(x(&doc, "#c"), 0.0, "incremental={incremental}");
        assert_eq!(x(&doc, "#b"), 10.0, "incremental={incremental}");
        assert_eq!(x(&doc, "#e"), 20.0, "incremental={incremental}");
        assert_eq!(x(&doc, "#a"), 30.0, "incremental={incremental}");
        assert_eq!(x(&doc, "#d"), 40.0, "incremental={incremental}");

        assert_eq!(
            ids_of(&doc, &layout_children(&doc, "#flex")),
            ["c", "b", "e", "a", "d"],
            "incremental={incremental}"
        );
    }
}

#[test]
fn grid_auto_placement_follows_order_modified_document_order() {
    const HTML: &str = r#"<html><body style="margin:0">
        <div id="grid" style="display:grid; grid-template-columns: 10px 10px; grid-auto-rows: 10px; width:400px;">
            <div id="a" style="order: 2"></div>
            <div id="b"></div>
            <div id="c" style="order: -1"></div>
            <div id="d"></div>
        </div>
    </body></html>"#;

    for incremental in [false, true] {
        let doc = make_doc(HTML, incremental);

        // Placement order: c, b, d, a -> cells (0,0) (10,0) (0,10) (10,10)
        assert_eq!((x(&doc, "#c"), y(&doc, "#c")), (0.0, 0.0));
        assert_eq!((x(&doc, "#b"), y(&doc, "#b")), (10.0, 0.0));
        assert_eq!((x(&doc, "#d"), y(&doc, "#d")), (0.0, 10.0));
        assert_eq!((x(&doc, "#a"), y(&doc, "#a")), (10.0, 10.0));

        assert_eq!(
            ids_of(&doc, &layout_children(&doc, "#grid")),
            ["c", "b", "d", "a"],
            "incremental={incremental}"
        );
    }
}

#[test]
fn abspos_children_of_flex_container_ignore_order() {
    const HTML: &str = r#"<html><body style="margin:0">
        <div id="flex" style="display:flex; position:relative; width:400px; height: 50px;">
            <div id="abs1" style="position:absolute; width:10px; height:10px; order: 5"></div>
            <div id="a" style="width:10px; height:10px; order: 1"></div>
            <div id="b" style="width:10px; height:10px;"></div>
            <div id="abs2" style="position:absolute; width:10px; height:10px; order: -5"></div>
        </div>
    </body></html>"#;

    for incremental in [false, true] {
        let doc = make_doc(HTML, incremental);

        // In-flow items: b (0) before a (1).
        assert_eq!(x(&doc, "#b"), 0.0, "incremental={incremental}");
        assert_eq!(x(&doc, "#a"), 10.0, "incremental={incremental}");

        // Abspos children key as 0 and keep source order among themselves:
        // abs1 stays before the in-flow 0-keyed item, abs2 stays after it.
        assert_eq!(
            ids_of(&doc, &layout_children(&doc, "#flex")),
            ["abs1", "b", "abs2", "a"],
            "incremental={incremental}"
        );

        // Static position of the abspos boxes: both sit at the start of the
        // container (they take no space in the flex line).
        assert_eq!(x(&doc, "#abs1"), 0.0, "incremental={incremental}");
        assert_eq!(x(&doc, "#abs2"), 0.0, "incremental={incremental}");
    }
}

/// Changing `order` via a restyle (here `:hover`) re-sorts the container's
/// `layout_children` in incremental mode without reconstructing any boxes.
///
/// A bare text node forces an anonymous block wrapper, whose identity we use
/// to detect box reconstruction of the container: reconstruction frees the
/// old anonymous block and allocates a new one.
#[test]
fn changing_order_via_restyle_resorts_without_reconstruction() {
    const HTML: &str = r#"<html><head><style>
        #flex:hover #c { order: -1; }
        #flex:hover #a { order: 1; }
    </style></head><body style="margin:0">
        <div id="flex" style="display:flex; width:400px; height:20px; font-size:10px; line-height:1;">
            <div id="a" style="width:10px; height:10px;"></div>
            <div id="b" style="width:10px; height:10px;"></div>
            <div id="c" style="width:10px; height:10px;"></div>
            x
        </div>
    </body></html>"#;

    let mut doc = make_doc(HTML, true);

    let initial = layout_children(&doc, "#flex");
    assert_eq!(ids_of(&doc, &initial), ["a", "b", "c", "<anon>"]);
    let anon = *initial.last().unwrap();
    let node_count = doc.tree().len();
    assert_eq!(x(&doc, "#a"), 0.0);
    assert_eq!(x(&doc, "#b"), 10.0);
    assert_eq!(x(&doc, "#c"), 20.0);
    let text_x = doc.get_node(anon).unwrap().final_layout().location.x;
    assert_eq!(text_x, 30.0);

    // Hover the container: c -> -1, a -> 1.
    doc.set_hover_to(5.0, 5.0);
    doc.resolve(0.0);

    let hovered = layout_children(&doc, "#flex");
    assert_eq!(ids_of(&doc, &hovered), ["c", "b", "<anon>", "a"]);
    assert_eq!(x(&doc, "#c"), 0.0);
    assert_eq!(x(&doc, "#b"), 10.0);
    let anon_layout = *doc.get_node(anon).unwrap().final_layout();
    assert_eq!(anon_layout.location.x, 20.0);
    assert_eq!(x(&doc, "#a"), 20.0 + anon_layout.size.width);
    assert_eq!(hovered[2], anon, "anonymous block was reconstructed");
    assert_eq!(doc.tree().len(), node_count, "nodes were (re)allocated");

    // Steady state: resolving again with no changes keeps the list.
    doc.resolve(0.0);
    assert_eq!(layout_children(&doc, "#flex"), hovered);

    // Unhover: back to source order.
    doc.set_hover_to(200.0, 200.0);
    doc.resolve(0.0);

    let unhovered = layout_children(&doc, "#flex");
    assert_eq!(unhovered, initial);
    assert_eq!(x(&doc, "#a"), 0.0);
    assert_eq!(x(&doc, "#b"), 10.0);
    assert_eq!(x(&doc, "#c"), 20.0);
    assert_eq!(doc.tree().len(), node_count, "nodes were (re)allocated");
}

/// Changing `order` through the attribute API (which conservatively marks the
/// element for reconstruction) also ends up sorted.
#[test]
fn changing_order_via_attribute_resorts() {
    let mut doc = make_doc(FLEX_HTML, true);
    assert_eq!(
        ids_of(&doc, &layout_children(&doc, "#flex")),
        ["c", "b", "e", "a", "d"]
    );

    let e = id(&doc, "#e");
    doc.mutate()
        .set_attribute(e, attr("style"), "width:10px; height:10px; order: -2");
    doc.resolve(0.0);

    assert_eq!(
        ids_of(&doc, &layout_children(&doc, "#flex")),
        ["e", "c", "b", "a", "d"]
    );
    assert_eq!(x(&doc, "#e"), 0.0);
    assert_eq!(x(&doc, "#c"), 10.0);
    assert_eq!(x(&doc, "#d"), 40.0);
}

#[test]
fn pseudo_elements_honour_order_in_flex_container() {
    const HTML: &str = r#"<html><head><style>
        #flex::before { content: ""; display:block; width:10px; height:10px; order: 1; }
        #flex::after { content: ""; display:block; width:10px; height:10px; order: -1; }
        #b { order: 1; }
        #flex:hover #a { order: 2; }
        #flex:hover #b { order: 0; }
    </style></head><body style="margin:0">
        <div id="flex" style="display:flex; width:400px; height:10px;">
            <div id="a" style="width:10px; height:10px;"></div>
            <div id="b" style="width:10px; height:10px;"></div>
        </div>
    </body></html>"#;

    for incremental in [false, true] {
        let mut doc = make_doc(HTML, incremental);
        let flex = doc.get_node(id(&doc, "#flex")).unwrap();
        let before = flex.before().expect("::before");
        let after = flex.after().expect("::after");

        // ::after (-1), a (0), ::before (1), b (1): ties keep ::before first.
        assert_eq!(
            layout_children(&doc, "#flex"),
            [after, id(&doc, "#a"), before, id(&doc, "#b")]
        );
        assert_eq!(doc.get_node(after).unwrap().final_layout().location.x, 0.0);
        assert_eq!(x(&doc, "#a"), 10.0);
        assert_eq!(
            doc.get_node(before).unwrap().final_layout().location.x,
            20.0
        );
        assert_eq!(x(&doc, "#b"), 30.0);

        // Hover: a -> 2, b -> 0. ::after (-1), b (0), ::before (1), a (2).
        doc.set_hover_to(5.0, 5.0);
        doc.resolve(0.0);
        assert_eq!(
            layout_children(&doc, "#flex"),
            [after, id(&doc, "#b"), before, id(&doc, "#a")]
        );
        assert_eq!(x(&doc, "#b"), 10.0);
        assert_eq!(x(&doc, "#a"), 30.0);

        // Unhover: ties (::before, b) are back in source order.
        doc.set_hover_to(200.0, 200.0);
        doc.resolve(0.0);
        assert_eq!(
            layout_children(&doc, "#flex"),
            [after, id(&doc, "#a"), before, id(&doc, "#b")]
        );
        assert_eq!(x(&doc, "#a"), 10.0);
        assert_eq!(x(&doc, "#b"), 30.0);
    }
}
