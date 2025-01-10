use crate::{node::NodeSpecificData, util::resolve_url, BaseDocument};
use markup5ever::local_name;

pub(crate) fn handle_mousedown(doc: &mut BaseDocument, target: usize, x: f32, y: f32) {
    let node = &mut doc.nodes[target];
    let Some(el) = node.raw_dom_data.downcast_element_mut() else {
        return;
    };

    let disabled = el.attr(local_name!("disabled")).is_some();
    if disabled {
        return;
    }

    if let NodeSpecificData::TextInput(ref mut text_input_data) = el.node_specific_data {
        let content_box_offset = taffy::Point {
            x: node.final_layout.padding.left + node.final_layout.border.left,
            y: node.final_layout.padding.top + node.final_layout.border.top,
        };

        let x = (x - content_box_offset.x) as f64 * doc.viewport.scale_f64();
        let y = (y - content_box_offset.y) as f64 * doc.viewport.scale_f64();
        text_input_data
            .editor
            .driver(&mut doc.font_ctx, &mut doc.layout_ctx)
            .move_to_point(x as f32, y as f32);

        doc.set_focus_to(target);
    } else if el.name.local == local_name!("input")
        && matches!(
            el.attr(local_name!("type")),
            Some("checkbox") | Some("radio")
        )
    {
        doc.set_focus_to(target);
    }
}

pub(crate) fn handle_click(doc: &mut BaseDocument, target: usize, _x: f32, _y: f32) {
    let node = &mut doc.nodes[target];
    let Some(el) = node.raw_dom_data.downcast_element_mut() else {
        return;
    };

    let disabled = el.attr(local_name!("disabled")).is_some();
    if disabled {
        return;
    }

    if el.name.local == local_name!("input")
        && matches!(el.attr(local_name!("type")), Some("checkbox"))
    {
        BaseDocument::toggle_checkbox(el);
    } else if el.name.local == local_name!("input")
        && matches!(el.attr(local_name!("type")), Some("radio"))
    {
        let node_id = node.id;
        let radio_set = el.attr(local_name!("name")).unwrap().to_string();
        BaseDocument::toggle_radio(doc, radio_set, node_id);
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
