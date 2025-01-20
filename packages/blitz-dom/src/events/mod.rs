mod ime;
mod keyboard;
mod mouse;

use blitz_traits::{DomEvent, DomEventData};

pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keypress;
use mouse::handle_mouseover;
pub(crate) use mouse::{handle_click, handle_mousedown, handle_mousemove};

use crate::BaseDocument;

pub(crate) fn handle_event(doc: &mut BaseDocument, event: &mut DomEvent) {
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
                event.request_redraw = true;
            }
        }
        DomEventData::MouseDown(event) => {
            handle_mousedown(doc, target_node_id, event.x, event.y);
        }
        DomEventData::MouseUp(_) => {}
        DomEventData::MouseOver(event) => {
            handle_mouseover(doc, target_node_id, event.x, event.y);
        }
        DomEventData::MouseLeave => {}
        DomEventData::Click(event) => {
            handle_click(doc, target_node_id, event.x, event.y);
        }
        DomEventData::KeyPress(event) => {
            handle_keypress(doc, target_node_id, event.clone());
        }
        DomEventData::Ime(event) => {
            handle_ime_event(doc, event.clone());
        }
    }
}
