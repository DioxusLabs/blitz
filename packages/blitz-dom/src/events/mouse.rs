use markup5ever::local_name;

use crate::{node::NodeSpecificData, util::resolve_url, BaseDocument};

pub(crate) fn handle_click(doc: &mut BaseDocument, target: usize, x: f32, y: f32) {
    let hit = doc.hit(x, y);
    if let Some(hit) = hit {
        assert!(hit.node_id == target);

        let node = &mut doc.nodes[hit.node_id];
        let content_box_offset = taffy::Point {
            x: node.final_layout.padding.left + node.final_layout.border.left,
            y: node.final_layout.padding.top + node.final_layout.border.top,
        };
        let Some(el) = node.raw_dom_data.downcast_element_mut() else {
            return;
        };

        let disabled = el.attr(local_name!("disabled")).is_some();
        if disabled {
            return;
        }

        if let NodeSpecificData::TextInput(ref mut text_input_data) = el.node_specific_data {
            let x = (hit.x - content_box_offset.x) as f64 * doc.viewport.scale_f64();
            let y = (hit.y - content_box_offset.y) as f64 * doc.viewport.scale_f64();
            text_input_data
                .editor
                .driver(&mut doc.font_ctx, &mut doc.layout_ctx)
                .move_to_point(x as f32, y as f32);

            doc.set_focus_to(hit.node_id);
        } else if el.name.local == local_name!("input")
            && matches!(el.attr(local_name!("type")), Some("checkbox"))
        {
            BaseDocument::toggle_checkbox(el);
            doc.set_focus_to(hit.node_id);
        } else if el.name.local == local_name!("input")
            && matches!(el.attr(local_name!("type")), Some("radio"))
        {
            let node_id = node.id;
            let radio_set = el.attr(local_name!("name")).unwrap().to_string();
            self.toggle_radio(radio_set, node_id);
            self.set_focus_to(hit.node_id);
        }
        // Clicking labels triggers click, and possibly input event, of associated input
        else if el.name.local == local_name!("label") {
            let node_id = node.id;
            if let Some(target_node_id) = doc
                .label_bound_input_elements(node_id)
                .first()
                .map(|n| n.id)
            {
                let target_node = doc.get_node_mut(target_node_id).unwrap();
                if let Some(target_element) = target_node.element_data_mut() {
                    BaseDocument::toggle_checkbox(target_element);
                }
                doc.set_focus_to(node_id);
            }
        } else if el.name.local == local_name!("a") {
            if let Some(href) = el.attr(local_name!("href")) {
                if let Some(url) = resolve_url(&doc.base_url, href) {
                    doc.navigation_provider.navigate_new_page(url.into());
                } else {
                    println!(
                        "{href} is not parseable as a url. : {base_url:?}",
                        base_url = doc.base_url
                    )
                }
            } else {
                println!("Clicked link without href: {:?}", el.attrs());
            }
        }
    }
}
