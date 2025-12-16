use blitz_traits::events::{BlitzFocusEvent, DomEvent, DomEventData};

use crate::BaseDocument;

pub(crate) fn generate_focus_events(
    doc: &mut BaseDocument,
    update_focus: &mut dyn FnMut(&mut BaseDocument),
    dispatch_event: &mut dyn FnMut(DomEvent),
) {
    // Update focus, tracking which node was focussed before and after
    let old_focus = doc.get_focussed_node_id();
    update_focus(doc);
    let new_focus = doc.get_focussed_node_id();

    if old_focus == new_focus {
        return;
    }

    if let Some(old_focus) = old_focus {
        dispatch_event(DomEvent::new(
            old_focus,
            DomEventData::Blur(BlitzFocusEvent),
        ));
        dispatch_event(DomEvent::new(
            old_focus,
            DomEventData::FocusOut(BlitzFocusEvent),
        ));
    }

    if let Some(new_focus) = new_focus {
        dispatch_event(DomEvent::new(
            new_focus,
            DomEventData::Focus(BlitzFocusEvent),
        ));
        dispatch_event(DomEvent::new(
            new_focus,
            DomEventData::FocusIn(BlitzFocusEvent),
        ));
    }
}
