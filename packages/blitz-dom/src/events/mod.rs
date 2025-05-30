mod driver;
mod ime;
mod keyboard;
mod mouse;

use blitz_traits::{DomEvent, DomEventData};
pub use driver::{EventDriver, EventHandler, NoopEventHandler};
pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keypress;
use mouse::handle_mouseup;
pub(crate) use mouse::{handle_click, handle_mousedown, handle_mousemove};

use crate::BaseDocument;

pub(crate) fn handle_event<F: FnMut(DomEvent)>(
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
                // TODO: request redraw
                // event_state.request_redraw();
            }
        }
        DomEventData::MouseDown(event) => {
            handle_mousedown(doc, target_node_id, event.x, event.y);
        }
        DomEventData::MouseUp(event) => {
            handle_mouseup(doc, target_node_id, event, dispatch_event);
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
            handle_ime_event(doc, event.clone());
        }
        DomEventData::Input(_) => {
            // Do nothing (no default action)
        }
    }
}
