//! Interaction state (hover/active/focus/mousedown) must never retain NodeIds
//! for layout-generated nodes (anonymous boxes, pseudo-elements), and must not
//! retain NodeIds for DOM nodes that have been removed from the node tree.
//!
//! Regression test for https://github.com/DioxusLabs/blitz/issues/545: the
//! hovered/active/focused node could be a pseudo-element or anonymous block
//! dropped during box reconstruction (or a removed DOM subtree). The stale
//! NodeId then panics with "invalid SlotMap key used" on the next
//! hover/active/focus update.
//!
//! Interaction state is canonicalized when stored: hits on layout-generated
//! nodes resolve to the nearest DOM ancestor (e.g. a pseudo-element's
//! originating element), whose id is stable across box-tree reconstruction.
//! State referencing genuinely removed DOM nodes is cleared.

use blitz_dom::{Document, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point,
        PointerCoords, PointerDetails, UiEvent,
    },
    shell::{ColorScheme, Viewport},
};
use markup5ever::{QualName, local_name, ns};
use std::sync::Arc;

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

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; }
    #target { width: 100px; height: 100px; }
    #target.with-pseudo:before { display: block; content: ""; width: 100px; height: 50px; }
</style></head>
<body><div id="target" class="with-pseudo"></div></body></html>
"#;

fn make_doc() -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

/// Hover the ::before pseudo-element of #target. The stored hover target must
/// be the *originating element* (#target), not the pseudo-element node itself.
fn hover_pseudo_region(doc: &mut HtmlDocument) -> blitz_traits::node_id::NodeId {
    let target_id = doc.query_selector("#target").unwrap().unwrap();
    let before_id = doc
        .get_node(target_id)
        .unwrap()
        .before()
        .expect("::before node should exist");
    assert_eq!(
        doc.hit(5.0, 5.0)
            .expect("hit should land somewhere")
            .node_id,
        before_id,
        "expected the hit-test to hit the ::before pseudo-element"
    );

    doc.set_hover_to(5.0, 5.0);
    let hover_id = doc.get_hover_node_id().expect("expected a hover node");
    assert_eq!(
        hover_id, target_id,
        "expected the stored hover target to be the pseudo's originating element"
    );
    hover_id
}

/// Remove the class that generates the ::before pseudo-element so that the
/// pseudo node is dropped on the next resolve.
fn remove_pseudo(doc: &mut HtmlDocument) {
    let target_id = doc.query_selector("#target").unwrap().unwrap();
    let class_name = QualName::new(None, ns!(), local_name!("class"));
    doc.mutate().clear_attribute(target_id, class_name);
    doc.resolve(0.0);
}

#[test]
fn hover_state_survives_hovered_pseudo_element_being_dropped() {
    let mut doc = make_doc();
    let hover_id = hover_pseudo_region(&mut doc);

    remove_pseudo(&mut doc);

    // The canonical hover target (#target) is a real DOM node and survives the
    // pseudo-element being dropped.
    assert_eq!(doc.get_hover_node_id(), Some(hover_id));
    assert!(doc.get_node(hover_id).is_some());
    // Must not panic with "invalid SlotMap key used"
    doc.set_hover_to(5.0, 5.0);
}

#[test]
fn active_state_survives_active_pseudo_element_being_dropped() {
    let mut doc = make_doc();
    let hover_id = hover_pseudo_region(&mut doc);
    doc.active_node();

    remove_pseudo(&mut doc);
    assert!(doc.get_node(hover_id).is_some());

    // Must not panic with "invalid SlotMap key used"
    doc.unactive_node();
}

#[test]
fn mousedown_state_survives_mousedown_pseudo_element_being_dropped() {
    let mut doc = make_doc();
    let target_id = doc.query_selector("#target").unwrap().unwrap();

    // Press the mouse on the pseudo-element. The stored mousedown target must
    // be the originating element.
    doc.handle_ui_event(UiEvent::PointerDown(pointer_event(
        5.0,
        5.0,
        MouseEventButtons::from(MouseEventButton::Main),
    )));
    assert_eq!(doc.get_mousedown_node_id(), Some(target_id));

    remove_pseudo(&mut doc);
    assert_eq!(doc.get_mousedown_node_id(), Some(target_id));

    // Drag with the button held (starts a selection drag from the mousedown
    // node). Must not panic with "invalid SlotMap key used".
    doc.handle_ui_event(UiEvent::PointerMove(pointer_event(
        20.0,
        20.0,
        MouseEventButtons::from(MouseEventButton::Main),
    )));
    doc.handle_ui_event(UiEvent::PointerUp(pointer_event(
        20.0,
        20.0,
        MouseEventButtons::None,
    )));
}

#[test]
fn hover_state_is_reresolved_when_hovered_element_is_removed() {
    let mut doc = make_doc();
    let hover_id = hover_pseudo_region(&mut doc);

    doc.mutate().remove_and_drop_node(hover_id);
    doc.resolve(0.0);
    assert!(doc.get_node(hover_id).is_none());

    // The stale id must be gone, and hover must have been re-resolved against
    // the new layout (the pointer is now over the <body>).
    let new_hover_id = doc.get_hover_node_id();
    assert_ne!(new_hover_id, Some(hover_id));
    if let Some(id) = new_hover_id {
        assert!(doc.get_node(id).is_some());
    }
    // Must not panic with "invalid SlotMap key used"
    doc.set_hover_to(5.0, 5.0);
}

#[test]
fn focus_state_is_cleared_when_focused_node_is_dropped() {
    let mut doc = make_doc();

    let target_id = doc.query_selector("#target").unwrap().unwrap();
    doc.set_focus_to(target_id);
    doc.mutate().remove_and_drop_node(target_id);
    doc.resolve(0.0);

    // Must not panic with "invalid SlotMap key used"
    doc.clear_focus();
}
