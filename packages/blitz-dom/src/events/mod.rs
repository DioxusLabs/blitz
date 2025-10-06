mod driver;
mod ime;
mod keyboard;
mod mouse;

use blitz_traits::events::{DomEvent, DomEventData};
pub use driver::{EventDriver, EventHandler, NoopEventHandler};
pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keypress;
use mouse::handle_mouseup;
pub(crate) use mouse::{handle_click, handle_mousedown, handle_mousemove};
use mouse::{handle_mouse_hover, handle_mouse_unhover};

use crate::BaseDocument;

pub(crate) fn handle_dom_event<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    event: &mut DomEvent,
    dispatch_event: F,
) {
    let target_node_id = event.target;

    match &event.data {
        DomEventData::MouseMove(mouse_event) => {
            let changed = handle_mousemove(
                doc,
                target_node_id,
                mouse_event.x,
                mouse_event.y,
                mouse_event.buttons,
            );
            if changed {
                doc.shell_provider.request_redraw();
            }
        }
        DomEventData::MouseDown(event) => {
            handle_mousedown(doc, target_node_id, event.x, event.y);
        }
        DomEventData::MouseUp(event) => {
            handle_mouseup(doc, target_node_id, event, dispatch_event);
        }
        DomEventData::MouseOver(mouse_event) | DomEventData::MouseEnter(mouse_event) => {
            handle_mouse_hover(doc, target_node_id, mouse_event.x, mouse_event.y);
        }
        DomEventData::MouseOut(mouse_event) => {
            let changed = handle_mouse_unhover(
                doc,
                target_node_id,
                false,
                Some(mouse_event.x),
                Some(mouse_event.y),
            );
            if changed {
                event.request_redraw = true;
            }
        }
        DomEventData::MouseLeave(mouse_event) => {
            handle_mouse_unhover(
                doc,
                target_node_id,
                true,
                Some(mouse_event.x),
                Some(mouse_event.y),
            );
        }
        DomEventData::Click(event) => {
            handle_click(doc, target_node_id, event, dispatch_event);
        }
        DomEventData::KeyDown(event) => {
            handle_keypress(doc, target_node_id, event.clone(), dispatch_event);
        }
        DomEventData::KeyPress(_) => {
            // Do nothing (no default action)
        }
        DomEventData::KeyUp(_) => {
            // Do nothing (no default action)
        }
        DomEventData::Ime(event) => {
            handle_ime_event(doc, event.clone(), dispatch_event);
        }
        DomEventData::Input(_) => {
            // Do nothing (no default action)
        }
    }
}
