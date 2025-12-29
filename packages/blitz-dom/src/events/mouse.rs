use std::{
    collections::VecDeque,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use blitz_traits::{
    events::{
        BlitzInputEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent,
        DomEvent, DomEventData, MouseEventButton, MouseEventButtons,
    },
    navigation::NavigationOptions,
};
use keyboard_types::Modifiers;
use markup5ever::local_name;

use crate::{
    BaseDocument,
    document::{DragMode, FlingState, PanSample, PanState, ScrollAnimationState},
    node::SpecialElementData,
};

use super::focus::generate_focus_events;

pub(crate) fn handle_mousemove<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target: usize,
    event: &BlitzPointerEvent,
    mut dispatch_event: F,
) -> bool {
    let x = event.x;
    let y = event.y;
    let buttons = event.buttons;

    let mut changed = doc.set_hover_to(x, y);

    // Check if we've moved enough to be considered a selection drag (2px threshold)
    if buttons != MouseEventButtons::None && doc.drag_mode == DragMode::None {
        let dx = x - doc.mousedown_position.x;
        let dy = y - doc.mousedown_position.y;
        if dx.abs() > 2.0 || dy.abs() > 2.0 {
            match event.id {
                BlitzPointerId::Mouse => {
                    doc.drag_mode = DragMode::Selecting;
                }
                BlitzPointerId::Finger(_) => {
                    doc.drag_mode = DragMode::Panning(PanState {
                        target,
                        last_x: event.screen_x,
                        last_y: event.screen_y,
                        samples: VecDeque::with_capacity(200),
                    });
                }
            }
        }
    }

    if let DragMode::Panning(state) = &mut doc.drag_mode {
        let dx = (event.screen_x - state.last_x) as f64;
        let dy = (event.screen_y - state.last_y) as f64;
        let time_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let target = state.target;
        state.last_x = event.screen_x;
        state.last_y = event.screen_y;

        state.samples.push_back(PanSample {
            time: time_ms,
            // TODO: account for scroll delta not applied due to clamping
            dx: dx as f32,
            dy: dy as f32,
        });

        // Remove samples older than 100ms
        if state.samples.len() > 50 && time_ms - state.samples.front().unwrap().time > 100 {
            let idx = state
                .samples
                .partition_point(|sample| time_ms - sample.time > 100);
            // FIXME: use truncate_front once stable
            for _ in 0..idx {
                state.samples.pop_front();
            }
        }

        let has_changed = doc.scroll_by(Some(target), dx, dy, &mut dispatch_event);
        return has_changed;
    }

    let Some(hit) = doc.hit(x, y) else {
        return changed;
    };

    if changed {
        dispatch_event(DomEvent::new(
            hit.node_id,
            DomEventData::MouseEnter(event.clone()),
        ));
    }

    if hit.node_id != target {
        return changed;
    }

    let node = &mut doc.nodes[target];
    let Some(el) = node.data.downcast_element_mut() else {
        // Handle text selection extension for non-element nodes
        if buttons != MouseEventButtons::None && doc.extend_text_selection_to_point(x, y) {
            changed = true;
        }
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
    } else if buttons != MouseEventButtons::None && doc.extend_text_selection_to_point(x, y) {
        changed = true;
    }

    changed
}

pub(crate) fn handle_mousedown(
    doc: &mut BaseDocument,
    _target: usize,
    x: f32,
    y: f32,
    mods: Modifiers,
    dispatch_event: &mut dyn FnMut(DomEvent),
) {
    // Compute click count using the previous mousedown position (before updating)
    // This handles both double-click detection and text input word/line selection
    // TODO: For text inputs, only increment click count if click maps to the same/similar caret position
    doc.click_count = if doc
        .last_mousedown_time
        .map(|t| t.elapsed() < Duration::from_millis(500))
        .unwrap_or(false)
        && (doc.mousedown_position.x - x).abs() <= 2.0
        && (doc.mousedown_position.y - y).abs() <= 2.0
    {
        doc.click_count + 1
    } else {
        1
    };

    // Update mousedown tracking for next click and selection drag detection
    doc.last_mousedown_time = Some(Instant::now());
    doc.mousedown_position = taffy::Point { x, y };
    doc.drag_mode = DragMode::None;
    doc.scroll_animation = ScrollAnimationState::None;

    let Some(hit) = doc.hit(x, y) else {
        // Clear text selection when clicking outside any element
        doc.clear_text_selection();
        return;
    };

    // Use hit.node_id for determining the actual clicked element.
    // This may differ from `target` for anonymous blocks (which are layout children
    // but not DOM children), so we use the hit result for text selection.
    let actual_target = hit.node_id;

    // Check what kind of element we're dealing with and extract needed info
    enum ClickTarget {
        TextInput {
            content_box_offset: taffy::Point<f32>,
        },
        Disabled,
        SelectableText,
    }

    let click_target = {
        let node = &doc.nodes[actual_target];
        match node.data.downcast_element() {
            Some(el) if el.has_attr(local_name!("disabled")) => ClickTarget::Disabled,
            Some(el) => {
                if let SpecialElementData::TextInput(ref text_input_data) = el.special_data {
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
                    ClickTarget::TextInput { content_box_offset }
                } else {
                    ClickTarget::SelectableText
                }
            }
            None => ClickTarget::SelectableText,
        }
    };

    match click_target {
        ClickTarget::Disabled => (),
        ClickTarget::SelectableText => {
            // Handle text selection for non-input elements
            if let Some((inline_root_id, byte_offset)) = doc.find_text_position(x, y) {
                doc.set_text_selection(inline_root_id, byte_offset, inline_root_id, byte_offset);
                doc.shell_provider.request_redraw();
            } else {
                doc.clear_text_selection();
            }
        }
        ClickTarget::TextInput { content_box_offset } => {
            // Clear general text selection when focusing a text input
            doc.clear_text_selection();

            let tx = (hit.x - content_box_offset.x) as f64 * doc.viewport.scale_f64();
            let ty = (hit.y - content_box_offset.y) as f64 * doc.viewport.scale_f64();

            // Now get mutable access to the text input
            let click_count = doc.click_count;
            let node = &mut doc.nodes[actual_target];
            let el = node.data.downcast_element_mut().unwrap();
            if let SpecialElementData::TextInput(ref mut text_input_data) = el.special_data {
                let mut font_ctx = doc.font_ctx.lock().unwrap();
                let mut driver = text_input_data
                    .editor
                    .driver(&mut font_ctx, &mut doc.layout_ctx);

                match click_count {
                    1 => {
                        if mods.shift() {
                            driver.shift_click_extension(tx as f32, ty as f32);
                        } else {
                            driver.move_to_point(tx as f32, ty as f32);
                        }
                    }
                    2 => driver.select_word_at_point(tx as f32, ty as f32),
                    _ => driver.select_hard_line_at_point(tx as f32, ty as f32),
                }

                drop(font_ctx);
            }

            generate_focus_events(
                doc,
                &mut |doc| {
                    doc.set_focus_to(hit.node_id);
                },
                dispatch_event,
            );
        }
    }
}

pub(crate) fn handle_mouseup<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target: usize,
    event: &BlitzPointerEvent,
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

    // Reset Document's drag state to DragMode::None, storing the state
    // locally for use within this function
    let drag_mode = doc.drag_mode.take();

    // Don't dispatch click if we were doing a text selection drag or panning
    // the document with a touch
    let do_click = drag_mode == DragMode::None;

    let time_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    if let DragMode::Panning(state) = &drag_mode {
        // Generate "fling"
        if let Some(last_sample) = state.samples.back()
            && time_ms - last_sample.time < 100
        {
            let idx = state
                .samples
                .partition_point(|sample| time_ms - sample.time > 100);

            // Compute pan_time. Will always be <= 100ms as we ignore samples older than that.
            let pan_start_time = state.samples[idx].time;
            let pan_time = (time_ms - pan_start_time) as f32;

            // Avoid division by 0
            if pan_time > 0.0 {
                let (pan_x, pan_y) = state
                    .samples
                    .iter()
                    .skip(idx)
                    .fold((0.0, 0.0), |(dx, dy), sample| {
                        (dx + sample.dx, dy + sample.dy)
                    });

                let x_velocity = if pan_x.abs() > pan_y.abs() {
                    pan_x / pan_time
                } else {
                    0.0
                };

                let y_velocity = if pan_y.abs() > pan_x.abs() {
                    pan_y / pan_time
                } else {
                    0.0
                };

                let fling = FlingState {
                    target: state.target,
                    last_seen_time: time_ms as f64,
                    x_velocity: x_velocity as f64, // * 16.6666,
                    y_velocity: y_velocity as f64, // * 16.6666,
                };

                doc.scroll_animation = ScrollAnimationState::Fling(fling);
                doc.shell_provider.request_redraw();
            }
        }
    }

    // Dispatch a click event
    if do_click && event.button == MouseEventButton::Main {
        dispatch_event(DomEvent::new(target, DomEventData::Click(event.clone())));
    }

    // Dispatch a context menu event
    if do_click && event.button == MouseEventButton::Secondary {
        dispatch_event(DomEvent::new(
            target,
            DomEventData::ContextMenu(event.clone()),
        ));
    }
}

pub(crate) fn handle_click(
    doc: &mut BaseDocument,
    target: usize,
    event: &BlitzPointerEvent,
    dispatch_event: &mut dyn FnMut(DomEvent),
) {
    let double_click_event = event.clone();

    let mut maybe_node_id = Some(target);
    let matched = 'matched: {
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
                break 'matched true;
            }

            if let SpecialElementData::TextInput(_) = el.special_data {
                break 'matched true;
            }

            match el.name.local {
                local_name!("input") if el.attr(local_name!("type")) == Some("checkbox") => {
                    let is_checked = BaseDocument::toggle_checkbox(el);
                    let value = is_checked.to_string();
                    dispatch_event(DomEvent::new(
                        node_id,
                        DomEventData::Input(BlitzInputEvent { value }),
                    ));
                    generate_focus_events(
                        doc,
                        &mut |doc| {
                            doc.set_focus_to(node_id);
                        },
                        dispatch_event,
                    );
                    break 'matched true;
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

                    generate_focus_events(
                        doc,
                        &mut |doc| {
                            doc.set_focus_to(node_id);
                        },
                        dispatch_event,
                    );

                    break 'matched true;
                }
                // Clicking labels triggers click, and possibly input event, of associated input
                local_name!("label") => {
                    if let Some(target_node_id) =
                        doc.label_bound_input_element(node_id).map(|n| n.id)
                    {
                        // Apply default click event action for target node
                        let target_node = doc.get_node_mut(target_node_id).unwrap();
                        let syn_event = target_node.synthetic_click_event_data(event.mods);
                        handle_click(doc, target_node_id, &syn_event, dispatch_event);
                        break 'matched true;
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
                        break 'matched true;
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

        // Didn't match anything
        false
    };

    // If nothing is matched then clear focus
    if !matched {
        generate_focus_events(doc, &mut |doc| doc.clear_focus(), dispatch_event);
    }

    // Dispatch double-click event if this is the second click in quick succession
    // (click_count was already computed in handle_mousedown)
    if doc.click_count == 2 {
        dispatch_event(DomEvent::new(
            target,
            DomEventData::DoubleClick(double_click_event),
        ));
    }
}

pub(crate) fn handle_wheel<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    _: usize,
    event: BlitzWheelEvent,
    mut dispatch_event: F,
) {
    let (scroll_x, scroll_y) = match event.delta {
        BlitzWheelDelta::Lines(x, y) => (x * 20.0, y * 20.0),
        BlitzWheelDelta::Pixels(x, y) => (x, y),
    };

    let has_changed = doc.scroll_by(
        doc.get_hover_node_id(),
        scroll_x,
        scroll_y,
        &mut dispatch_event,
    );
    if has_changed {
        doc.shell_provider.request_redraw();
    }
}
