use winit::event::Ime;

use crate::BaseDocument;

pub(crate) fn handle_ime_event(doc: &mut BaseDocument, event: Ime) {
    if let Some(node_id) = doc.focus_node_id {
        let node = &mut doc.nodes[node_id];
        let text_input_data = node
            .raw_dom_data
            .downcast_element_mut()
            .and_then(|el| el.text_input_data_mut());
        if let Some(input_data) = text_input_data {
            let editor = &mut input_data.editor;
            let mut driver = editor.driver(&mut doc.font_ctx, &mut doc.layout_ctx);

            match event {
                Ime::Enabled => { /* Do nothing */ }
                Ime::Disabled => {
                    driver.clear_compose();
                }
                Ime::Commit(text) => {
                    driver.insert_or_replace_selection(&text);
                }
                Ime::Preedit(text, cursor) => {
                    if text.is_empty() {
                        driver.clear_compose();
                    } else {
                        driver.set_compose(&text, cursor);
                    }
                }
            }
            println!("Sent ime event to {}", node_id);
        }
    }
}
