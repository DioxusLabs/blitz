mod ime;
mod keyboard;
mod mouse;

use blitz_traits::{DomEvent, DomEventData};
pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keydown;
pub(crate) use mouse::{handle_click, handle_mousedown};

use crate::BaseDocument;

pub(crate) fn handle_event(doc: &mut BaseDocument, event: DomEvent) {
    let target_node_id = event.current_target.unwrap();

    match event.data {
        DomEventData::MouseDown(event) => {
            handle_mousedown(doc, target_node_id, event.x, event.y);
        }
        DomEventData::MouseUp(_) => {}
        DomEventData::Hover => {}
        DomEventData::Click(event) => {
            handle_click(doc, target_node_id, event.x, event.y);
        }
        DomEventData::Input(_) => {}
        DomEventData::KeyDown(event) => {
            handle_keydown(doc, target_node_id, event);
        }
        DomEventData::KeyUp(_) => {}
        DomEventData::KeyPress(_) => {}
        DomEventData::Ime(event) => {
            handle_ime_event(doc, event);
        }
        DomEventData::Event(_) => {}
    }
}
