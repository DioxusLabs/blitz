use blitz_traits::events::{BlitzImeEvent, DomEvent};

use crate::BaseDocument;

pub(crate) fn handle_ime_event<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    event: BlitzImeEvent,
    mut dispatch_event: F,
) {
    if let Some(node_id) = doc.focus_node_id {
        let node = &mut doc.nodes[node_id];
        let text_input_data = node
            .data
            .downcast_element_mut()
            .and_then(|el| el.text_input_data_mut());
        if let Some(input_data) = text_input_data {
            let generated_events = input_data.apply_ime_event(
                &mut doc.font_ctx.lock().unwrap(),
                &mut doc.layout_ctx,
                event,
            );

            for generated_event in generated_events {
                doc.apply_generated_text_input_event(node_id, generated_event, &mut dispatch_event);
            }

            #[cfg(feature = "tracing")]
            tracing::debug!(node_id = ?node_id, "Sent ime event");
        }
    }
}
