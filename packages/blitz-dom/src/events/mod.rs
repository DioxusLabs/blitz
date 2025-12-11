mod driver;
mod ime;
mod keyboard;
mod mouse;

use blitz_traits::events::{DomEvent, DomEventData, UiEvent};
pub use driver::{EventDriver, EventHandler, NoopEventHandler};
pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keypress;
use mouse::handle_mouseup;
pub(crate) use mouse::{handle_click, handle_mousedown, handle_mousemove};

use crate::BaseDocument;

pub(crate) fn handle_dom_event<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    event: &mut DomEvent,
    dispatch_event: F,
) {
    let target_node_id = event.target;

    // Handle forwarding event sub-document
    let node = &mut doc.nodes[target_node_id];
    let pos = node.absolute_position(0.0, 0.0);
    let mut set_focus = false;
    if let Some(sub_doc) = node.subdoc_mut() {
        // TODO: eliminate clone
        let ui_event = match event.data.clone() {
            DomEventData::MouseMove(mut mouse_event) => {
                mouse_event.x -= pos.x;
                mouse_event.y -= pos.y;
                Some(UiEvent::MouseMove(mouse_event))
            }
            DomEventData::MouseDown(mut mouse_event) => {
                mouse_event.x -= pos.x;
                mouse_event.y -= pos.y;
                set_focus = true;
                Some(UiEvent::MouseDown(mouse_event))
            }
            DomEventData::MouseUp(mut mouse_event) => {
                mouse_event.x -= pos.x;
                mouse_event.y -= pos.y;
                set_focus = true;
                Some(UiEvent::MouseUp(mouse_event))
            }
            DomEventData::MouseEnter(_) => None,
            DomEventData::MouseLeave(_) => None,
            DomEventData::MouseOver(_) => None,
            DomEventData::MouseOut(_) => None,
            DomEventData::KeyDown(data) => Some(UiEvent::KeyDown(data)),
            DomEventData::KeyUp(data) => Some(UiEvent::KeyUp(data)),
            DomEventData::Ime(data) => Some(UiEvent::Ime(data)),
            DomEventData::KeyPress(_) => None,
            DomEventData::Click(_) => None,
            DomEventData::ContextMenu(_) => None,
            DomEventData::DoubleClick(_) => None,
            DomEventData::Input(_) => None,
        };

        if let Some(ui_event) = ui_event {
            sub_doc.handle_ui_event(ui_event);
            doc.shell_provider.request_redraw();
        }

        if set_focus {
            doc.set_focus_to(target_node_id);
        }

        return;
    }

    match &event.data {
        DomEventData::MouseMove(mouse_event) => {
            let changed = handle_mousemove(
                doc,
                target_node_id,
                mouse_event.x,
                mouse_event.y,
                mouse_event.buttons,
                mouse_event,
                dispatch_event
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
        DomEventData::ContextMenu(_) => {
            // TODO: Open context menu
        },
        DomEventData::DoubleClick(_) => {
            // Do nothing (no default action)
        },
        DomEventData::MouseEnter(_) => {
            // Do nothing (no default action)
        },
        DomEventData::MouseLeave(_) => {
            // Do nothing (no default action)
        },
        DomEventData::MouseOver(_) => {
            // Do nothing (no default action)
        },
        DomEventData::MouseOut(_) => {
            // Do nothing (no default action)
        },
    }
}
