use blitz_traits::events::{BlitzImeEvent, BlitzInputEvent, DomEvent, DomEventData};

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
            let editor = &mut input_data.editor;
            let mut font_ctx = doc.font_ctx.lock().unwrap();
            let mut driver = editor.driver(&mut font_ctx, &mut doc.layout_ctx);

            match event {
                BlitzImeEvent::Enabled => { /* Do nothing */ }
                BlitzImeEvent::Disabled => {
                    driver.clear_compose();
                }
                BlitzImeEvent::Commit(text) => {
                    driver.insert_or_replace_selection(&text);
                    let value = input_data.editor.raw_text().to_string();
                    dispatch_event(DomEvent::new(
                        node_id,
                        DomEventData::Input(BlitzInputEvent { value }),
                    ));
                }
                BlitzImeEvent::Preedit(text, cursor) => {
                    if text.is_empty() {
                        driver.clear_compose();
                    } else {
                        driver.set_compose(&text, cursor);
                    }
                }
            }
            println!("Sent ime event to {node_id}");
        }
    }
}
