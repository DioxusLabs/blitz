mod driver;
mod focus;
mod ime;
mod keyboard;
mod pointer;

use crate::util::Point;
use blitz_traits::events::{DomEvent, DomEventData, PointerCoords, UiEvent};
pub use driver::{EventDriver, EventHandler, NoopEventHandler};
use focus::generate_focus_events;
pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keypress;
pub(crate) use pointer::{DragMode, ScrollAnimationState};
use pointer::{handle_click, handle_pointerdown, handle_pointermove, handle_pointerup};

use crate::{BaseDocument, events::pointer::handle_wheel};

fn adjust_coords_for_subdocument(
    coords: &mut PointerCoords,
    offset: Point<f32>,
    viewport_scroll: Point<f64>,
) {
    coords.page_x -= offset.x - viewport_scroll.x as f32;
    coords.page_y -= offset.y - viewport_scroll.y as f32;
    coords.client_x -= offset.x;
    coords.client_y -= offset.y;
}

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
            DomEventData::PointerMove(mut event) => {
                adjust_coords_for_subdocument(&mut event.coords, pos, viewport_scroll);
                Some(UiEvent::PointerMove(event))
            }
            DomEventData::PointerDown(mut event) => {
                adjust_coords_for_subdocument(&mut event.coords, pos, viewport_scroll);
                set_focus = true;
                Some(UiEvent::PointerDown(event))
            }
            DomEventData::PointerUp(mut event) => {
                adjust_coords_for_subdocument(&mut event.coords, pos, viewport_scroll);
                set_focus = true;
                Some(UiEvent::PointerUp(event))
            }

            // Enter/leave events will be recreated by sub-document's event driver
            // based move events
            DomEventData::PointerEnter(_) => None,
            DomEventData::PointerLeave(_) => None,
            DomEventData::PointerOver(_) => None,
            DomEventData::PointerOut(_) => None,

            // Mouse events will be recreated by sub-document's event driver
            // based pointer events
            DomEventData::MouseMove(_) => None,
            DomEventData::MouseDown(_) => None,
            DomEventData::MouseUp(_) => None,
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
        DomEventData::PointerMove(event) => {
            let changed = handle_pointermove(doc, target_node_id, event, dispatch_event);
            if changed {
                doc.shell_provider.request_redraw();
            }
        }
        DomEventData::MouseMove(_) => {
            // Do nothing (handled in PointerMove)
        }
        DomEventData::PointerDown(event) => {
            handle_pointerdown(
                doc,
                target_node_id,
                event.page_x(),
                event.page_y(),
                event.mods,
                &mut dispatch_event,
            );
        }
        DomEventData::MouseDown(_) => {
            // Do nothing (handled in PointerDown)
        }
        DomEventData::PointerUp(event) => {
            handle_pointerup(doc, target_node_id, event, dispatch_event);
        }
        DomEventData::MouseUp(_) => {
            // Do nothing (handled in PointerUp)
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
        DomEventData::PointerEnter(_) => {
            // Do nothing (no default action)
        }
        DomEventData::PointerLeave(_) => {
            // Do nothing (no default action)
        }
        DomEventData::PointerOver(_) => {
            // Do nothing (no default action)
        }
        DomEventData::PointerOut(_) => {
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
