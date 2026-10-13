use blitz_traits::events::{BlitzImeEvent, DomEvent};

use crate::BaseDocument;

pub(crate) fn handle_ime_event<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    event: BlitzImeEvent,
    dispatch_event: F,
) {
    if let Some(node_id) = doc.focus_node_id {
        let node = &mut doc.nodes[node_id];
        let Some(element) = node.data.downcast_element_mut() else {
            return;
        };
        let max_length = element.text_input_max_length();
        if let Some(input_data) = element.text_input_data_mut() {
            let generated_event = input_data.apply_ime_event(
                &mut doc.font_ctx.lock().unwrap(),
                &mut doc.layout_ctx,
                max_length,
                event,
            );

            if let Some(generated_event) = generated_event {
                doc.apply_generated_text_input_event(node_id, generated_event, dispatch_event);
            }

            #[cfg(feature = "tracing")]
            tracing::debug!(node_id = ?node_id, "Sent ime event");
        }
    }
}
