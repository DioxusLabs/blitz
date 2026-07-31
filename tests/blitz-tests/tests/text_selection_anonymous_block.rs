//! Drag text selection must work over bare text that gets wrapped in an
//! anonymous block during box-tree construction.
//!
//! Mirrors the `mutations` example: `<body>Outer<div>Inner</div></body>`.
//! The bare "Outer" text next to a block sibling is wrapped in an anonymous
//! block, so text hits report the anonymous block as the hit node while the
//! pointer event's target is the canonicalized DOM node (the body). The
//! selection drag path must not require these to be the same node.

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

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style> body { margin: 0; } </style></head>
<body>Outer<div id="inner">Inner</div></body></html>
"#;

/// A point inside the "Outer" text (near its start).
const OUTER_POS: (f32, f32) = (2.0, 8.0);

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

/// Verify that OUTER_POS hits text inside an anonymous block. Returns false
/// (skip the test) if no usable font is available, in which case text
/// measures 0x0 and text hits are impossible.
fn hit_is_text_in_anonymous_block(doc: &HtmlDocument) -> bool {
    let Some(hit) = doc.hit(OUTER_POS.0, OUTER_POS.1) else {
        return false;
    };
    if !hit.is_text {
        return false;
    }
    assert!(
        doc.get_node(hit.node_id).unwrap().is_anonymous(),
        "expected the text hit to be inside an anonymous block"
    );
    true
}

/// Press at `from` and drag to `to` with the main button held.
fn drag(doc: &mut HtmlDocument, from: (f32, f32), to: (f32, f32)) {
    let held = MouseEventButtons::from(MouseEventButton::Main);
    doc.handle_ui_event(UiEvent::PointerDown(pointer_event(from.0, from.1, held)));
    doc.handle_ui_event(UiEvent::PointerMove(pointer_event(to.0, to.1, held)));
    doc.handle_ui_event(UiEvent::PointerUp(pointer_event(
        to.0,
        to.1,
        MouseEventButtons::None,
    )));
}

fn node_center(doc: &HtmlDocument, node_id: NodeId) -> (f32, f32) {
    let rect = doc.get_client_bounding_rect(node_id).unwrap();
    (
        (rect.x + rect.width / 2.0) as f32,
        (rect.y + rect.height / 2.0) as f32,
    )
}

/// Regression test: dragging across "Outer" (wrapped in an anonymous block)
/// must extend the selection, even though the pointer event's canonical
/// target (the body) differs from the precise hit node (the anonymous block).
#[test]
fn drag_selection_within_anonymous_block_wrapped_text() {
    let mut doc = make_doc();
    if !hit_is_text_in_anonymous_block(&doc) {
        eprintln!("skipping: no usable font (text measures 0x0)");
        return;
    }

    drag(&mut doc, OUTER_POS, (35.0, OUTER_POS.1));

    assert!(
        doc.has_text_selection(),
        "expected an active text selection"
    );
    let selected = doc.get_selected_text().expect("expected selected text");
    assert!(
        !selected.is_empty() && "Outer".contains(&selected),
        "expected a non-empty part of \"Outer\" to be selected, got {selected:?}"
    );
}

/// Dragging from the anonymous-block-wrapped "Outer" into the "Inner" div
/// must produce a selection spanning both inline roots.
#[test]
fn drag_selection_extends_from_anonymous_block_into_sibling_block() {
    let mut doc = make_doc();
    if !hit_is_text_in_anonymous_block(&doc) {
        eprintln!("skipping: no usable font (text measures 0x0)");
        return;
    }

    let inner_id = doc.query_selector("#inner").unwrap().expect("#inner");
    let inner_center = node_center(&doc, inner_id);
    drag(&mut doc, OUTER_POS, inner_center);

    assert!(
        doc.has_text_selection(),
        "expected an active text selection"
    );
    let selected = doc.get_selected_text().expect("expected selected text");
    let (outer_part, inner_part) = selected
        .split_once(' ')
        .expect("expected selection to span both inline roots");
    assert!(
        !outer_part.is_empty() && "Outer".ends_with(outer_part),
        "expected the first part to be a suffix of \"Outer\", got {outer_part:?}"
    );
    assert!(
        !inner_part.is_empty() && "Inner".starts_with(inner_part),
        "expected the second part to be a prefix of \"Inner\", got {inner_part:?}"
    );
}
