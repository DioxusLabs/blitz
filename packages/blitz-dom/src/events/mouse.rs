use std::time::{Duration, Instant};

use blitz_traits::{
    events::{
        BlitzInputEvent, BlitzMouseButtonEvent, DomEvent, DomEventData, MouseEventButton,
        MouseEventButtons,
    },
    navigation::NavigationOptions,
};
use markup5ever::local_name;

use crate::{BaseDocument, node::SpecialElementData};

pub(crate) fn handle_mousemove<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target: usize,
    x: f32,
    y: f32,
    buttons: MouseEventButtons,
    event: &BlitzMouseButtonEvent,
    mut dispatch_event: F
) -> bool {
    let mut changed = doc.set_hover_to(x, y);

    let Some(hit) = doc.hit(x, y) else {
        return changed;
    };

    if changed {
        dispatch_event(DomEvent::new(hit.node_id, DomEventData::MouseEnter(event.clone())));
    }

    if hit.node_id != target {
        return changed;
    }

    let node = &mut doc.nodes[target];
    let Some(el) = node.data.downcast_element_mut() else {
        return changed;
    };

    let disabled = el.attr(local_name!("disabled")).is_some();
    if disabled {
        return changed;
    }

    if let SpecialElementData::TextInput(ref mut text_input_data) = el.special_data {
        if buttons == MouseEventButtons::None {
            return changed;
        }

        let mut content_box_offset = taffy::Point {
            x: node.final_layout.padding.left + node.final_layout.border.left,
            y: node.final_layout.padding.top + node.final_layout.border.top,
        };
        if !text_input_data.is_multiline {
            let layout = text_input_data.editor.try_layout().unwrap();
            let content_box_height = node.final_layout.content_box_height();
            let input_height = layout.height() / layout.scale();
            let y_offset = ((content_box_height - input_height) / 2.0).max(0.0);

            content_box_offset.y += y_offset;
        }

        let x = (hit.x - content_box_offset.x) as f64 * doc.viewport.scale_f64();
        let y = (hit.y - content_box_offset.y) as f64 * doc.viewport.scale_f64();

        text_input_data
            .editor
            .driver(&mut doc.font_ctx.lock().unwrap(), &mut doc.layout_ctx)
            .extend_selection_to_point(x as f32, y as f32);

        changed = true;
    }

    changed
}

pub(crate) fn handle_mousedown(doc: &mut BaseDocument, target: usize, x: f32, y: f32) {
    let Some(hit) = doc.hit(x, y) else {
        return;
    };
    if hit.node_id != target {
        return;
    }

    let node = &mut doc.nodes[target];
    let Some(el) = node.data.downcast_element_mut() else {
        return;
    };

    let disabled = el.attr(local_name!("disabled")).is_some();
    if disabled {
        return;
    }

    if let SpecialElementData::TextInput(ref mut text_input_data) = el.special_data {
        let mut content_box_offset = taffy::Point {
            x: node.final_layout.padding.left + node.final_layout.border.left,
            y: node.final_layout.padding.top + node.final_layout.border.top,
        };
        if !text_input_data.is_multiline {
            let layout = text_input_data.editor.try_layout().unwrap();
            let content_box_height = node.final_layout.content_box_height();
            let input_height = layout.height() / layout.scale();
            let y_offset = ((content_box_height - input_height) / 2.0).max(0.0);

            content_box_offset.y += y_offset;
        }

        let x = (hit.x - content_box_offset.x) as f64 * doc.viewport.scale_f64();
        let y = (hit.y - content_box_offset.y) as f64 * doc.viewport.scale_f64();

        text_input_data
            .editor
            .driver(&mut doc.font_ctx.lock().unwrap(), &mut doc.layout_ctx)
            .move_to_point(x as f32, y as f32);

        doc.set_focus_to(hit.node_id);
    }
}

pub(crate) fn handle_mouseup<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target: usize,
    event: &BlitzMouseButtonEvent,
    mut dispatch_event: F,
) {
    if doc.devtools().highlight_hover {
        let mut node = doc.get_node(target).unwrap();
        if event.button == MouseEventButton::Secondary {
            if let Some(parent_id) = node.layout_parent.get() {
                node = doc.get_node(parent_id).unwrap();
            }
        }
        doc.debug_log_node(node.id);
        doc.devtools_mut().highlight_hover = false;
        return;
    }

    // Determine whether to dispatch a click event
    let do_click = true;
    // let do_click = doc.mouse_down_node.is_some_and(|mouse_down_id| {
    //     // Anonymous node ids are unstable due to tree reconstruction. So we compare the id
    //     // of the first non-anonymous ancestor.
    //     mouse_down_id == target
    //         || doc.non_anon_ancestor_if_anon(mouse_down_id) == doc.non_anon_ancestor_if_anon(target)
    // });

    // Dispatch a click event
    if do_click && event.button == MouseEventButton::Main {
        dispatch_event(DomEvent::new(target, DomEventData::Click(event.clone())));
    }

    // Dispatch a context menu event
    if do_click && event.button == MouseEventButton::Secondary {
        dispatch_event(DomEvent::new(target, DomEventData::ContextMenu(event.clone())));
    }
}

pub(crate) fn handle_click<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target: usize,
    event: &BlitzMouseButtonEvent,
    mut dispatch_event: F,
) {
    let mut maybe_node_id = Some(target);
    while let Some(node_id) = maybe_node_id {
        let maybe_element = {
            let node = &mut doc.nodes[node_id];
            node.data.downcast_element_mut()
        };

        let Some(el) = maybe_element else {
            maybe_node_id = doc.nodes[node_id].parent;
            continue;
        };

        let disabled = el.attr(local_name!("disabled")).is_some();
        if disabled {
            return;
        }

        if let SpecialElementData::TextInput(_) = el.special_data {
            return;
        }

        match el.name.local {
            local_name!("input") if el.attr(local_name!("type")) == Some("checkbox") => {
                let is_checked = BaseDocument::toggle_checkbox(el);
                let value = is_checked.to_string();
                dispatch_event(DomEvent::new(
                    node_id,
                    DomEventData::Input(BlitzInputEvent { value }),
                ));
                doc.set_focus_to(node_id);
                return;
            }
            local_name!("input") if el.attr(local_name!("type")) == Some("radio") => {
                let radio_set = el.attr(local_name!("name")).unwrap().to_string();
                BaseDocument::toggle_radio(doc, radio_set, node_id);

                // TODO: make input event conditional on value actually changing
                let value = String::from("true");
                dispatch_event(DomEvent::new(
                    node_id,
                    DomEventData::Input(BlitzInputEvent { value }),
                ));

                BaseDocument::set_focus_to(doc, node_id);

                return;
            }
            // Clicking labels triggers click, and possibly input event, of associated input
            local_name!("label") => {
                if let Some(target_node_id) = doc.label_bound_input_element(node_id).map(|n| n.id) {
                    // Apply default click event action for target node
                    let target_node = doc.get_node_mut(target_node_id).unwrap();
                    let syn_event = target_node.synthetic_click_event_data(event.mods);
                    handle_click(doc, target_node_id, &syn_event, dispatch_event);
                    return;
                }
            }
            local_name!("a") => {
                if let Some(href) = el.attr(local_name!("href")) {
                    if let Some(url) = doc.url.resolve_relative(href) {
                        doc.navigation_provider.navigate_to(NavigationOptions::new(
                            url,
                            String::from("text/plain"),
                            doc.id(),
                        ));
                    } else {
                        println!("{href} is not parseable as a url. : {:?}", *doc.url)
                    }
                    return;
                } else {
                    println!("Clicked link without href: {:?}", el.attrs());
                }
            }
            local_name!("input")
                if el.is_submit_button() || el.attr(local_name!("type")) == Some("submit") =>
            {
                if let Some(form_owner) = doc.controls_to_form.get(&node_id) {
                    doc.submit_form(*form_owner, node_id);
                }
            }
            #[cfg(feature = "file_input")]
            local_name!("input") if el.attr(local_name!("type")) == Some("file") => {
                use crate::qual_name;
                //TODO: Handle accept attribute https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Attributes/accept by passing an appropriate filter
                let multiple = el.attr(local_name!("multiple")).is_some();
                let files = doc.shell_provider.open_file_dialog(multiple, None);

                if let Some(file) = files.first() {
                    el.attrs
                        .set(qual_name!("value", html), &file.to_string_lossy());
                }
                let text_content = match files.len() {
                    0 => "No Files Selected".to_string(),
                    1 => files
                        .first()
                        .unwrap()
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    x => format!("{x} Files Selected"),
                };

                if files.is_empty() {
                    el.special_data = SpecialElementData::None;
                } else {
                    el.special_data = SpecialElementData::FileInput(files.into())
                }
                let child_label_id = doc.nodes[node_id].children[1];
                let child_text_id = doc.nodes[child_label_id].children[0];
                let text_data = doc.nodes[child_text_id]
                    .text_data_mut()
                    .expect("Text data not found");
                text_data.content = text_content;
            }
            _ => {}
        }

        // No match. Recurse up to parent.
        maybe_node_id = doc.nodes[node_id].parent;
    }

    // If nothing is matched then clear focus
    doc.clear_focus();

    // Assumed double click time to be less than 500ms, although may be system-dependant?
    if doc.dbl_click_first_time.map(|t| t.elapsed() < Duration::from_millis(500)).unwrap_or(false)  {
        dispatch_event(DomEvent::new(target, DomEventData::DoubleClick(event.clone())));
        doc.dbl_click_first_time = None;
    } else {
        doc.dbl_click_first_time = Some(Instant::now());
    }
}