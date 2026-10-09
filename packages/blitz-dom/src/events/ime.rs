use blitz_traits::events::{BlitzImeEvent, DomEvent};

use crate::BaseDocument;

pub(crate) fn handle_ime_event<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    event: BlitzImeEvent,
    dispatch_event: F,
) {
    let reveals = doc.reveal_typed_password.is_some();
    apply_ime_event(doc, event, reveals, dispatch_event);
}

/// Inserts pasted text into the focused text input, as an input method commits text. A password
/// field never shows pasted text in the clear.
pub(crate) fn handle_paste<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    text: String,
    dispatch_event: F,
) {
    apply_ime_event(doc, BlitzImeEvent::Commit(text), false, dispatch_event);
}

fn apply_ime_event<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    event: BlitzImeEvent,
    reveals: bool,
    dispatch_event: F,
) {
    if let Some(node_id) = doc.focus_node_id {
        let node = &mut doc.nodes[node_id];
        let text_input_data = node
            .data
            .downcast_element_mut()
            .and_then(|el| el.text_input_data_mut());
        if let Some(input_data) = text_input_data {
            let generated_event = input_data.apply_ime_event(&mut doc.text, event, reveals);

            if let Some(generated_event) = generated_event {
                doc.note_password_reveal(node_id, web_time::Instant::now());
                doc.apply_generated_text_input_event(node_id, generated_event, dispatch_event);
            }

            #[cfg(feature = "tracing")]
            tracing::debug!(node_id = ?node_id, "Sent ime event");
        }
    }
}
