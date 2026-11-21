use crate::scrolling::{
    ScrollAmount, ScrollBehavior, ScrollOverflow, ScrollRequest, ScrollSource, ScrollTarget,
};
use crate::util::Point;
use crate::{BaseDocument, node::GeneratedTextInputEvent, util::ACTION_MOD};
use blitz_traits::node_id::NodeId;
use blitz_traits::{
    SmolStr,
    events::{BlitzInputEvent, BlitzKeyEvent, DomEventData},
};
use keyboard_types::{Key, Modifiers, NamedKey};
use markup5ever::local_name;
use style_dom::ElementState;

const KEYBOARD_SCROLL_LINE_STEP: f64 = 40.0;
const KEYBOARD_SCROLL_PAGE_FACTOR: f64 = 0.875;

pub(super) enum KeyboardOrTextInputEvent {
    KeyPress(BlitzKeyEvent),
    AppleStandardKeyBinding(SmolStr),
}

pub(crate) fn handle_key_or_input_event(
    doc: &mut BaseDocument,
    target: NodeId,
    event: KeyboardOrTextInputEvent,
) {
    if let KeyboardOrTextInputEvent::KeyPress(event) = &event {
        if event.key == Key::Named(NamedKey::Tab) {
            let backwards = event.modifiers.contains(Modifiers::SHIFT);
            if backwards {
                doc.focus_prev_node();
            } else {
                doc.focus_next_node();
            }
            return;
        }

        // Handle copy (Ctrl+C/Cmd+C) for text selection when no text input is focused
        if event.state.is_pressed() {
            let action_mod = event.modifiers.contains(ACTION_MOD);
            if action_mod {
                if let Key::Character(c) = &event.key {
                    if c.to_lowercase() == "c" {
                        // Check if we have a text selection (and no focused text input)
                        let has_focused_text_input = doc.focus_node_id.is_some_and(|id| {
                            doc.get_node(id)
                                .and_then(|n| n.element_data())
                                .is_some_and(|e| e.text_input_data().is_some())
                        });

                        if !has_focused_text_input {
                            if let Some(text) = doc.get_selected_text() {
                                let _ = doc.shell_provider.set_clipboard_text(text);
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(node_id) = doc.focus_node_id {
        let is_enabled_text_input = !is_node_disabled(doc, node_id)
            && doc
                .get_node(node_id)
                .and_then(|n| n.element_data())
                .is_some_and(|e| e.text_input_data().is_some());

        if is_enabled_text_input {
            if target != node_id {
                return;
            }

            if let Some(input_data) = doc.nodes[node_id]
                .element_data_mut()
                .and_then(|e| e.text_input_data_mut())
            {
                let generated_event = match event {
                    KeyboardOrTextInputEvent::KeyPress(blitz_key_event) => input_data
                        .apply_keypress_event(
                            &mut doc.font_ctx.lock().unwrap(),
                            &mut doc.layout_ctx,
                            &*doc.shell_provider,
                            blitz_key_event,
                        ),
                    KeyboardOrTextInputEvent::AppleStandardKeyBinding(command) => input_data
                        .apply_apple_standard_keybinding(
                            &mut doc.font_ctx.lock().unwrap(),
                            &mut doc.layout_ctx,
                            &*doc.shell_provider,
                            &command,
                        ),
                };

                if let Some(generated_event) = generated_event {
                    doc.apply_generated_text_input_event(node_id, generated_event);
                }
            }
            return;
        }
    }

    if let KeyboardOrTextInputEvent::KeyPress(key_event) = &event {
        handle_keyboard_scroll(doc, key_event);
    }
}

fn is_node_displayed(doc: &BaseDocument, node_id: NodeId) -> bool {
    doc.get_node(node_id).is_some_and(|node| {
        node.flags.is_in_document()
            && node
                .display_style()
                .is_some_and(|display| !display.is_none())
    })
}

fn is_node_disabled(doc: &BaseDocument, node_id: NodeId) -> bool {
    doc.get_node(node_id).is_some_and(|node| {
        node.element_state().contains(ElementState::DISABLED)
            || node
                .element_data()
                .is_some_and(|el| el.has_attr(local_name!("disabled")))
    })
}

fn canonical_scroll_target(doc: &BaseDocument, target: ScrollTarget) -> ScrollTarget {
    match target {
        ScrollTarget::Node(node_id)
            if doc.try_root_element().is_some_and(|el| el.id == node_id) =>
        {
            ScrollTarget::Viewport
        }
        other => other,
    }
}

fn effective_vertical_scroll_target(
    doc: &BaseDocument,
    start: ScrollTarget,
    dir_y: f64,
) -> Option<ScrollTarget> {
    let mut current = start;
    loop {
        let target = canonical_scroll_target(doc, current);
        let (offset, min, max) = doc.scroll_state(target, false);
        let can_scroll = if dir_y > 0.0 {
            offset.y < max.y
        } else {
            offset.y > min.y
        };
        if can_scroll {
            return Some(target);
        }

        let ScrollTarget::Node(node_id) = target else {
            return None;
        };
        let node = doc.nodes.get(node_id)?;
        if node.is_scroll_container() && node.contains_overscroll().y {
            return None;
        }
        current = node
            .parent
            .map_or(ScrollTarget::Viewport, ScrollTarget::Node);
    }
}

fn scroll_target_visible_height(doc: &BaseDocument, target: ScrollTarget) -> f64 {
    match target {
        ScrollTarget::Viewport => {
            let scale = doc.viewport.scale() as f64;
            doc.viewport.window_size.1 as f64 / scale
        }
        ScrollTarget::Node(node_id) => doc
            .get_node(node_id)
            .map_or(0.0, |node| node.client_height() as f64),
    }
}

fn handle_keyboard_scroll(doc: &mut BaseDocument, key_event: &BlitzKeyEvent) {
    if key_event.is_composing || !key_event.state.is_pressed() {
        return;
    }

    if key_event
        .modifiers
        .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::META)
    {
        return;
    }

    let shift = key_event.modifiers.contains(Modifiers::SHIFT);
    let start_target = doc
        .focus_node_id
        .filter(|&id| is_node_displayed(doc, id))
        .map_or(ScrollTarget::Viewport, ScrollTarget::Node);

    enum ScrollKeyAction {
        By(f64, f64),
        Page(f64),
        Edge(bool),
    }

    let action = match &key_event.key {
        Key::Named(NamedKey::ArrowUp) if !shift => {
            ScrollKeyAction::By(0.0, -KEYBOARD_SCROLL_LINE_STEP)
        }
        Key::Named(NamedKey::ArrowDown) if !shift => {
            ScrollKeyAction::By(0.0, KEYBOARD_SCROLL_LINE_STEP)
        }
        Key::Named(NamedKey::ArrowLeft) if !shift => {
            ScrollKeyAction::By(-KEYBOARD_SCROLL_LINE_STEP, 0.0)
        }
        Key::Named(NamedKey::ArrowRight) if !shift => {
            ScrollKeyAction::By(KEYBOARD_SCROLL_LINE_STEP, 0.0)
        }
        Key::Named(NamedKey::PageUp) if !shift => ScrollKeyAction::Page(-1.0),
        Key::Named(NamedKey::PageDown) if !shift => ScrollKeyAction::Page(1.0),
        Key::Named(NamedKey::Home) if !shift => ScrollKeyAction::Edge(false),
        Key::Named(NamedKey::End) if !shift => ScrollKeyAction::Edge(true),
        Key::Character(s) if s == " " => {
            if doc.focus_node_id.is_some_and(|id| {
                doc.is_space_activatable_element(id)
                    && is_node_displayed(doc, id)
                    && !is_node_disabled(doc, id)
            }) {
                return;
            }
            ScrollKeyAction::Page(if shift { -1.0 } else { 1.0 })
        }
        _ => return,
    };

    // `ScrollAmount::By` adds directly to scroll offsets (+y scrolls down, +x scrolls right),
    // unlike `scroll_chain_by` which negates wheel deltas.
    let has_changed = match action {
        ScrollKeyAction::By(dx, dy) => doc.scroll(ScrollRequest {
            target: start_target,
            amount: ScrollAmount::By(Point { x: dx, y: dy }),
            overflow: ScrollOverflow::Chain,
            source: ScrollSource::User,
            behavior: ScrollBehavior::Instant,
            interrupt_animation: true,
        }),
        ScrollKeyAction::Page(dir_y) => {
            let Some(effective_target) = effective_vertical_scroll_target(doc, start_target, dir_y)
            else {
                return;
            };
            let visible_height = scroll_target_visible_height(doc, effective_target);
            let step =
                (visible_height * KEYBOARD_SCROLL_PAGE_FACTOR).max(KEYBOARD_SCROLL_LINE_STEP);
            doc.scroll(ScrollRequest {
                target: start_target,
                amount: ScrollAmount::By(Point {
                    x: 0.0,
                    y: dir_y * step,
                }),
                overflow: ScrollOverflow::Chain,
                source: ScrollSource::User,
                behavior: ScrollBehavior::Instant,
                interrupt_animation: true,
            })
        }
        ScrollKeyAction::Edge(to_end) => {
            let dir_y = if to_end { 1.0 } else { -1.0 };
            let Some(effective_target) = effective_vertical_scroll_target(doc, start_target, dir_y)
            else {
                return;
            };
            let (current, min, max) = doc.scroll_state(effective_target, false);
            let target_y = if to_end { max.y } else { min.y };
            doc.scroll(ScrollRequest {
                target: effective_target,
                amount: ScrollAmount::To(Point {
                    x: current.x,
                    y: target_y,
                }),
                overflow: ScrollOverflow::Clamp,
                source: ScrollSource::User,
                behavior: ScrollBehavior::Instant,
                interrupt_animation: true,
            })
        }
    };

    if has_changed {
        doc.shell_provider.request_redraw();
    }
}

impl BaseDocument {
    pub(crate) fn is_space_activatable_element(&self, node_id: NodeId) -> bool {
        let Some(node) = self.get_node(node_id) else {
            return false;
        };
        let Some(el) = node.element_data() else {
            return false;
        };

        match el.name.local {
            local_name!("button") | local_name!("select") => true,
            local_name!("input") => el.attr(local_name!("type")).is_some_and(|t| {
                [
                    "button", "submit", "reset", "checkbox", "radio", "file", "image", "color",
                    "range",
                ]
                .iter()
                .any(|kind| t.eq_ignore_ascii_case(kind))
            }),
            local_name!("summary") => node.parent.is_some_and(|parent_id| {
                let parent = &self.nodes[parent_id];
                parent
                    .data
                    .is_element_with_tag_name(&local_name!("details"))
                    && parent.children.iter().copied().find(|&child_id| {
                        self.nodes[child_id]
                            .data
                            .is_element_with_tag_name(&local_name!("summary"))
                    }) == Some(node_id)
            }),
            _ => false,
        }
    }

    pub(crate) fn apply_generated_text_input_event(
        &mut self,
        node_id: NodeId,
        event: GeneratedTextInputEvent,
    ) {
        let node = &mut self.nodes[node_id];
        let element_data = node
            .element_data_mut()
            .expect("apply_generated_text_input_event called on a node that is not an element");
        let input_data = element_data
            .text_input_data_mut()
            .expect("apply_generated_text_input_event called on a node that is not a text input");

        match event {
            GeneratedTextInputEvent::Input => {
                let value = input_data.editor.raw_text().to_string();
                self.queue_ua_event(node_id, DomEventData::Input(BlitzInputEvent { value }));
                self.shell_provider.request_redraw();
            }
            GeneratedTextInputEvent::Select | GeneratedTextInputEvent::PreEditChange => {
                self.shell_provider.request_redraw();
            }
            GeneratedTextInputEvent::Submit => {
                // TODO: Generate submit event that can be handled by script
                implicit_form_submission(self, node_id);
            }
        }
    }
}

/// https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#field-that-blocks-implicit-submission
fn implicit_form_submission(doc: &BaseDocument, text_target: NodeId) {
    let Some(form_owner_id) = doc.controls_to_form.get(&text_target) else {
        return;
    };
    if doc
        .controls_to_form
        .iter()
        .filter(|(_control_id, form_id)| *form_id == form_owner_id)
        .filter_map(|(control_id, _)| doc.nodes[*control_id].element_data())
        .filter(|element_data| {
            element_data.attr(local_name!("type")).is_some_and(|t| {
                matches!(
                    t,
                    "text"
                        | "search"
                        | "email"
                        | "url"
                        | "tel"
                        | "password"
                        | "date"
                        | "month"
                        | "week"
                        | "time"
                        | "datetime-local"
                        | "number"
                )
            })
        })
        .count()
        > 1
    {
        return;
    }

    doc.submit_form(*form_owner_id, *form_owner_id);
}
