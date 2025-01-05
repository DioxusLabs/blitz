mod ime;
mod keyboard;
mod mouse;

use blitz_traits::{DomEvent, DomEventData};
pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keypress;
pub(crate) use mouse::handle_click;

use crate::BaseDocument;

pub(crate) fn handle_event(doc: &mut BaseDocument, event: DomEvent) {
    let target_node_id = event.target;

    match event.data {
        DomEventData::MouseDown(_) => {}
        DomEventData::MouseUp(_) => {}
        DomEventData::Hover => {}
        DomEventData::Click(event) => {
            handle_click(doc, target_node_id, event.x, event.y);
        }
        DomEventData::KeyPress(event) => {
            handle_keypress(doc, target_node_id, event);
        }
        DomEventData::Ime(event) => {
            handle_ime_event(doc, event);
        }
    }
}
