use blitz_traits::events::{BlitzTouchEvent, DomEvent};
use crate::BaseDocument;
use keyboard_types::Modifiers;

/// Handle touch start events
pub(crate) fn handle_touch_start(
    doc: &mut BaseDocument,
    target_node_id: usize,
    x: f32,
    y: f32,
) {
    println!("🟢 RUST: TouchStart at ({:.1}, {:.1}) on node {}", x, y, target_node_id);
    // For now, just treat touch start similar to mouse down
    // This can be expanded later with touch-specific functionality
    crate::events::mouse::handle_mousedown(doc, target_node_id, x, y);
}

/// Handle touch end events
pub(crate) fn handle_touch_end<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target_node_id: usize,
    event: &BlitzTouchEvent,
    dispatch_event: F,
) {
    println!("🔴 RUST: TouchEnd at ({:.1}, {:.1}) on node {}", event.x, event.y, target_node_id);
    // For now, just treat touch end similar to mouse up
    // This can be expanded later with touch-specific functionality
    let mouse_event = blitz_traits::events::BlitzMouseButtonEvent {
        x: event.x,
        y: event.y,
        buttons: blitz_traits::events::MouseEventButtons::Primary,
        mods: Modifiers::empty(),
        button: blitz_traits::events::MouseEventButton::Main,
    };
    crate::events::mouse::handle_mouseup(doc, target_node_id, &mouse_event, dispatch_event);
}

/// Handle touch move events
pub(crate) fn handle_touch_move(
    doc: &mut BaseDocument,
    target_node_id: usize,
    x: f32,
    y: f32,
) -> bool {
    println!("🟠 RUST: TouchMove to ({:.1}, {:.1}) on node {}", x, y, target_node_id);
    // For now, just treat touch move similar to mouse move
    // This can be expanded later with touch-specific functionality
    crate::events::mouse::handle_mousemove(
        doc,
        target_node_id,
        x,
        y,
        blitz_traits::events::MouseEventButtons::Primary,
    )
}

/// Handle touch cancel events
pub(crate) fn handle_touch_cancel<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target_node_id: usize,
    event: &BlitzTouchEvent,
    dispatch_event: F,
) {
    println!("❌ RUST: TouchCancel at ({:.1}, {:.1}) on node {}", event.x, event.y, target_node_id);
    // For now, just treat touch cancel similar to touch end
    handle_touch_end(doc, target_node_id, event, dispatch_event);
}
