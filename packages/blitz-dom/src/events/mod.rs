mod ime;
mod keyboard;
mod mouse;

use blitz_traits::{DomEvent, DomEventData};

pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keypress;
pub(crate) use mouse::{handle_click, handle_mousedown, handle_mousemove};
use mouse::{handle_mouse_hover, handle_mouse_unhover};

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
