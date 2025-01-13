mod ime;
mod keyboard;
mod mouse;

use blitz_traits::{DomEvent, DomEventData};
pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keydown;
pub(crate) use mouse::{handle_blur, handle_click, handle_mousedown, handle_mousemove};

use crate::BaseDocument;

pub(crate) fn handle_event(doc: &mut BaseDocument, event: &mut DomEvent) {
    let target_node_id = event.current_target.unwrap();

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
        DomEventData::Hover => {}
        DomEventData::Click(event) => {
            handle_click(doc, target_node_id, event.x, event.y);
        }
        DomEventData::Focus => {
            doc.focus_node(target_node_id);
        }
        DomEventData::Blur => {
            handle_blur(doc, target_node_id);
        }
        DomEventData::Input(_) => {}
        DomEventData::KeyDown(event) => {
            handle_keydown(doc, target_node_id, event.clone());
        }
        DomEventData::KeyUp(_) => {}
        DomEventData::KeyPress(_) => {}
        DomEventData::Ime(event) => {
            handle_ime_event(doc, event.clone());
        }
        DomEventData::Event(_) => {}
    }
}
