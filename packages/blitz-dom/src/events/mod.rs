mod driver;
mod focus;
mod ime;
mod keyboard;
mod mouse;

use blitz_traits::events::{DomEvent, DomEventData, UiEvent};
pub use driver::{EventDriver, EventHandler, NoopEventHandler};
use focus::generate_focus_events;
pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keypress;
use mouse::handle_mouseup;
pub(crate) use mouse::{handle_click, handle_mousedown, handle_mousemove};

use crate::{BaseDocument, events::mouse::handle_wheel};

pub(crate) fn handle_dom_event<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    event: &mut DomEvent,
    mut dispatch_event: F,
) {
    let target_node_id = event.target;

    // Handle forwarding event sub-document
    let node = &mut doc.nodes[target_node_id];
    let pos = node.absolute_position(0.0, 0.0);
    let mut set_focus = false;
    if let Some(sub_doc) = node.subdoc_mut() {
        let viewport_scroll = sub_doc.inner().viewport_scroll();
        // TODO: eliminate clone
        let ui_event = match event.data.clone() {
            DomEventData::MouseMove(mut mouse_event) => {
                mouse_event.page_x -= pos.x - viewport_scroll.x as f32;
                mouse_event.page_y -= pos.y - viewport_scroll.y as f32;
                mouse_event.client_x -= pos.x;
                mouse_event.client_y -= pos.y;
                Some(UiEvent::MouseMove(mouse_event))
            }
            DomEventData::MouseDown(mut mouse_event) => {
                mouse_event.page_x -= pos.x - viewport_scroll.x as f32;
                mouse_event.page_y -= pos.y - viewport_scroll.y as f32;
                mouse_event.client_x -= pos.x;
                mouse_event.client_y -= pos.y;
                set_focus = true;
                Some(UiEvent::MouseDown(mouse_event))
            }
            DomEventData::MouseUp(mut mouse_event) => {
                mouse_event.page_x -= pos.x - viewport_scroll.x as f32;
                mouse_event.page_y -= pos.y - viewport_scroll.y as f32;
                mouse_event.client_x -= pos.x;
                mouse_event.client_y -= pos.y;
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
            DomEventData::Wheel(data) => Some(UiEvent::Wheel(data)),
            DomEventData::Scroll(_) => None,
            DomEventData::Focus(_) => None,
            DomEventData::Blur(_) => None,
            DomEventData::FocusIn(_) => None,
            DomEventData::FocusOut(_) => None,
        };

        if let Some(ui_event) = ui_event {
            sub_doc.handle_ui_event(ui_event);
            doc.shell_provider.request_redraw();
        }

        if set_focus {
            generate_focus_events(
                doc,
                &mut |doc| {
                    doc.set_focus_to(target_node_id);
                },
                &mut dispatch_event,
            );
        }

        return;
    }

    match &event.data {
        DomEventData::MouseMove(mouse_event) => {
            let changed = handle_mousemove(doc, target_node_id, mouse_event, dispatch_event);
            if changed {
                doc.shell_provider.request_redraw();
            }
        }
        DomEventData::MouseDown(event) => {
            handle_mousedown(
                doc,
                target_node_id,
                event.page_x,
                event.page_y,
                event.mods,
                &mut dispatch_event,
            );
        }
        DomEventData::MouseUp(event) => {
            handle_mouseup(doc, target_node_id, event, dispatch_event);
        }
        DomEventData::Click(event) => {
            handle_click(doc, target_node_id, event, &mut dispatch_event);
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
        }
        DomEventData::DoubleClick(_) => {
            // Do nothing (no default action)
        }
        DomEventData::MouseEnter(_) => {
            // Do nothing (no default action)
        }
        DomEventData::MouseLeave(_) => {
            // Do nothing (no default action)
        }
        DomEventData::MouseOver(_) => {
            // Do nothing (no default action)
        }
        DomEventData::MouseOut(_) => {
            // Do nothing (no default action)
        }
        DomEventData::Scroll(_) => {
            // Handled elsewhere
        }
        DomEventData::Wheel(event) => {
            handle_wheel(doc, target_node_id, event.clone(), dispatch_event);
        }
        DomEventData::Focus(_) => {
            // Do nothing (no default action)
        }
        DomEventData::Blur(_) => {
            // Do nothing (no default action)
        }
        DomEventData::FocusIn(_) => {
            // Do nothing (no default action)
        }
        DomEventData::FocusOut(_) => {
            // Do nothing (no default action)
        }
    }
}
