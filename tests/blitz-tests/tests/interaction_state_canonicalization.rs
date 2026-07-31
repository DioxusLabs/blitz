//! Persistent interaction state (hover/mousedown/active/focus) must only ever
//! reference DOM nodes, never layout-generated nodes (anonymous boxes, pseudo
//! elements) whose NodeIds are invalidated by box-tree reconstruction.
//!
//! Hit-test results are canonicalized when stored: a hit on an anonymous block
//! resolves to its containing element, and a hit inside a pseudo-element
//! subtree resolves to the originating element. This means interaction state
//! survives reconstruction by construction (the canonical ids are stable), and
//! hover is additionally re-resolved against fresh layout at the end of each
//! `resolve` pass so that layout shifts under a stationary pointer update it.

use blitz_dom::{Document, DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point,
        PointerCoords, PointerDetails, UiEvent,
    },
    shell::{ColorScheme, Viewport},
};
use std::sync::Arc;

fn make_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn node_id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().expect(selector)
}

fn pointer_event(x: f32, y: f32, buttons: MouseEventButtons) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button: MouseEventButton::Main,
        buttons,
        mods: Default::default(),
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    }
}

/// A block container with mixed inline/block children: the inline-block span
/// is wrapped in an anonymous block during box-tree construction. (An
/// inline-block with explicit dimensions is used rather than bare text so the
/// test doesn't depend on font availability/metrics.) The area to the right of
/// the span still falls within the anonymous block, so hits there resolve to
/// the anonymous block node.
const ANON_HTML: &str = r#"<!DOCTYPE html>
<html><head><style> body { margin: 0; } </style></head>
<body><div id="container" style="width:300px;"><span id="inline" style="display:inline-block; width:100px; height:30px;"></span><div id="block" style="height:50px;"></div></div></body></html>
"#;

/// (x, y) inside the anonymous block wrapping the inline-block span, but to
/// the right of the span itself (so the hit is the anonymous block).
const ANON_POS: (f32, f32) = (250.0, 5.0);

/// Sanity-check that the hit-test at ANON_POS really does hit an anonymous
/// block whose canonical target is #container (otherwise the tests below
/// prove nothing).
fn assert_hit_is_anonymous(doc: &HtmlDocument) {
    let container_id = node_id(doc, "#container");
    let hit = doc.hit(ANON_POS.0, ANON_POS.1).expect("hit expected");
    let hit_node = doc.get_node(hit.node_id).unwrap();
    assert!(
        hit_node.is_anonymous(),
        "expected the hit-test to hit an anonymous block"
    );
    assert_eq!(
        doc.nearest_non_anonymous_ancestor(hit.node_id),
        Some(container_id)
    );
}

#[test]
fn hover_over_anonymous_block_targets_containing_element() {
    let mut doc = make_doc(ANON_HTML);
    assert_hit_is_anonymous(&doc);

    doc.set_hover_to(ANON_POS.0, ANON_POS.1);
    let hover_id = doc.get_hover_node_id().expect("expected a hover node");
    assert_eq!(hover_id, node_id(&doc, "#container"));
    assert!(!doc.get_node(hover_id).unwrap().is_anonymous());
}

#[test]
fn hover_survives_box_tree_reconstruction() {
    let mut doc = make_doc(ANON_HTML);
    assert_hit_is_anonymous(&doc);

    doc.set_hover_to(ANON_POS.0, ANON_POS.1);
    let hover_id = doc.get_hover_node_id().expect("expected a hover node");

    // Force full reconstruction on every resolve: all anonymous blocks are
    // deallocated and recreated with new ids.
    doc.set_incremental_layout(false);
    doc.resolve(0.0);
    doc.resolve(0.0);

    // The canonical hover target is a real DOM node, so it survives.
    assert_eq!(doc.get_hover_node_id(), Some(hover_id));
    assert!(doc.get_node(hover_id).is_some());
}

#[test]
fn mousedown_over_anonymous_block_survives_box_tree_reconstruction() {
    let mut doc = make_doc(ANON_HTML);
    assert_hit_is_anonymous(&doc);
    let container_id = node_id(&doc, "#container");

    doc.handle_ui_event(UiEvent::PointerDown(pointer_event(
        ANON_POS.0,
        ANON_POS.1,
        MouseEventButtons::from(MouseEventButton::Main),
    )));
    assert_eq!(doc.get_mousedown_node_id(), Some(container_id));

    // Reconstruct while the button is held.
    doc.set_incremental_layout(false);
    doc.resolve(0.0);
    assert_eq!(doc.get_mousedown_node_id(), Some(container_id));

    // Continuing the drag must not panic on a stale id.
    doc.handle_ui_event(UiEvent::PointerMove(pointer_event(
        ANON_POS.0 - 30.0,
        ANON_POS.1,
        MouseEventButtons::from(MouseEventButton::Main),
    )));
    doc.handle_ui_event(UiEvent::PointerUp(pointer_event(
        ANON_POS.0 - 30.0,
        ANON_POS.1,
        MouseEventButtons::None,
    )));
}

#[test]
fn active_state_over_anonymous_block_survives_box_tree_reconstruction() {
    let mut doc = make_doc(ANON_HTML);
    assert_hit_is_anonymous(&doc);

    doc.set_hover_to(ANON_POS.0, ANON_POS.1);
    doc.active_node();

    doc.set_incremental_layout(false);
    doc.resolve(0.0);

    // Must not panic with "invalid SlotMap key used"
    doc.unactive_node();
}

#[test]
fn hover_is_refreshed_after_layout_shift() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
<html><head><style> body { margin: 0; } </style></head>
<body>
    <div id="a" style="height:100px;"></div>
    <div id="b" style="height:100px;"></div>
</body></html>
"#,
    );
    let a_id = node_id(&doc, "#a");
    let b_id = node_id(&doc, "#b");

    doc.set_hover_to(50.0, 50.0);
    assert_eq!(doc.get_hover_node_id(), Some(a_id));

    // Remove #a: #b moves up underneath the (stationary) pointer. Hover must
    // be re-resolved during resolve() without any new pointer event.
    doc.mutate().remove_and_drop_node(a_id);
    doc.resolve(0.0);
    assert_eq!(doc.get_hover_node_id(), Some(b_id));
}
