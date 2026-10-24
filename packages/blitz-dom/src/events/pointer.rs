use blitz_traits::node_id::NodeId;
use stylo_taffy::CopyStyleGetters;

use web_time::{Duration, Instant};

use blitz_traits::{
    events::{
        BlitzInputEvent, BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent,
        DomEvent, DomEventData, MouseEventButton, MouseEventButtons,
    },
    navigation::NavigationOptions,
};
use keyboard_types::Modifiers;
use markup5ever::local_name;
use style::values::computed::{Overflow, TouchAction, UserSelect};
use style_dom::ElementState;
use taffy::AbsoluteAxis;

use crate::{
    BaseDocument,
    node::{ScrollbarRef, SpecialElementData},
    scrolling::{CarriedFling, FlingState, ScrollAnimationState},
    util::Point,
};

use super::focus::generate_focus_events;
use super::velocity::VelocityTracker;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PanState {
    pub(crate) target: NodeId,
    pub(crate) last_x: f32,
    pub(crate) last_y: f32,
    /// Whether horizontal panning is permitted by the `touch-action` property.
    pub(crate) allow_x: bool,
    /// Whether vertical panning is permitted by the `touch-action` property.
    pub(crate) allow_y: bool,
    tracker: VelocityTracker,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollbarDragState {
    /// The thumb being dragged
    pub(crate) scrollbar: ScrollbarRef,
    /// Last pointer position along the drag axis, in page coordinates
    pub(crate) last_pos: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DragMode {
    /// We are not currently dragging
    None,
    /// We are currently dragging a selection (probably mouse)
    Selecting,
    /// We are currently panning the document with a drag (probably touch)
    Panning(PanState),
    /// We are currently dragging a scrollbar thumb
    ScrollbarDrag(ScrollbarDragState),
}

impl DragMode {
    pub(crate) fn take(&mut self) -> DragMode {
        std::mem::replace(self, DragMode::None)
    }
}

impl PanState {
    fn update(&mut self, now: Instant, screen_x: f32, screen_y: f32) -> (f64, f64) {
        // Constrain panning to the axes permitted by the `touch-action` property. Positions are
        // still tracked on both axes so that deltas remain correct after a disallowed movement.
        let dx = if self.allow_x {
            (screen_x - self.last_x) as f64
        } else {
            0.0
        };
        let dy = if self.allow_y {
            (screen_y - self.last_y) as f64
        } else {
            0.0
        };
        self.last_x = screen_x;
        self.last_y = screen_y;

        self.tracker.record(now, dx, dy);

        (dx, dy)
    }

    fn generate_fling(&self, now: Instant, carried: Option<&CarriedFling>) -> Option<FlingState> {
        let mut velocity = self.tracker.velocity(now)?;
        velocity = Point {
            x: if velocity.x.abs() > velocity.y.abs() {
                velocity.x
            } else {
                0.0
            },
            y: if velocity.y.abs() > velocity.x.abs() {
                velocity.y
            } else {
                0.0
            },
        };
        if velocity.x.hypot(velocity.y) < 50.0 {
            return None;
        }
        if let Some(carried) = carried {
            velocity = carried.boost(now, velocity);
        }
        let scale = (8000.0 / velocity.x.hypot(velocity.y)).min(1.0);
        velocity.x *= scale;
        velocity.y *= scale;
        Some(FlingState::new(self.target, now, velocity))
    }
}

/// Compute which axes a touch pan gesture starting on `node_id` is allowed to scroll, according to
/// the `touch-action` property.
///
/// Per the Pointer Events spec the effective behaviour is the intersection of the `touch-action`
/// values of the target and its ancestors up to and including the nearest ancestor that implements
/// the pan (the nearest scroll container which can actually scroll on that axis), so the walk stops
/// per-axis at that scroller: `touch-action` values on elements *above* it do not restrict pans it
/// handles. `touch-action: none` blocks panning entirely, `pan-x`/`pan-y` restrict it to a single
/// axis, and `auto` / `manipulation` permit both.
fn touch_action_pan_axes(doc: &BaseDocument, node_id: NodeId) -> (bool, bool) {
    let pan_x_flags = TouchAction::AUTO | TouchAction::MANIPULATION | TouchAction::PAN_X;
    let pan_y_flags = TouchAction::AUTO | TouchAction::MANIPULATION | TouchAction::PAN_Y;

    let mut allow_x = true;
    let mut allow_y = true;
    // Whether the nearest scroller for the axis has been reached (its own `touch-action` counts,
    // but its ancestors' do not).
    let mut done_x = false;
    let mut done_y = false;
    let mut current = Some(node_id);
    while let Some(id) = current {
        let node = &doc.nodes[id];
        if let Some(style) = node.primary_styles() {
            let touch_action = style.clone_touch_action();
            if !done_x {
                allow_x &= touch_action.intersects(pan_x_flags);
            }
            if !done_y {
                allow_y &= touch_action.intersects(pan_y_flags);
            }

            let scrolls_x = matches!(style.clone_overflow_x(), Overflow::Scroll | Overflow::Auto)
                && node.final_layout().scroll_width() > 0.0;
            let scrolls_y = matches!(style.clone_overflow_y(), Overflow::Scroll | Overflow::Auto)
                && node.final_layout().scroll_height() > 0.0;
            done_x |= scrolls_x;
            done_y |= scrolls_y;

            if (done_x || !allow_x) && (done_y || !allow_y) {
                break;
            }
        }
        current = node.parent;
    }

    (allow_x, allow_y)
}

pub(crate) fn handle_pointermove<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target: NodeId,
    event: &BlitzPointerEvent,
    mut dispatch_event: F,
) -> bool {
    let x = event.page_x();
    let y = event.page_y();
    let buttons = event.buttons;

    let mut changed = doc.set_hover_to(x, y);

    // Check if we've moved enough to be considered a selection drag (2px threshold)
    if buttons != MouseEventButtons::None && doc.drag_mode == DragMode::None {
        let dx = x - doc.mousedown_position.x;
        let dy = y - doc.mousedown_position.y;
        if dx.abs() > 2.0 || dy.abs() > 2.0 {
            match event.id {
                BlitzPointerId::Mouse | BlitzPointerId::Pen => {
                    if let Some(mousedown_node_id) = doc.mousedown_node_id {
                        let node = &doc.nodes[mousedown_node_id];
                        if let Some(style) = node.primary_styles() {
                            let user_select = style.clone_user_select();
                            if user_select == UserSelect::None {
                                // Do nothing. Continue with rest of function
                            } else if user_select == UserSelect::Auto {
                                if let Some(parent) = node.parent {
                                    let node = &doc.nodes[parent];
                                    if let Some(style) = node.primary_styles() {
                                        let user_select = style.clone_user_select();
                                        if user_select == UserSelect::None {
                                            // Do nothing. Continue with rest of function
                                        } else {
                                            doc.drag_mode = DragMode::Selecting;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                BlitzPointerId::Finger(_) => {
                    let (allow_x, allow_y) = touch_action_pan_axes(doc, target);
                    // If `touch-action` forbids panning on both axes (e.g. `touch-action: none`)
                    // there is nothing to scroll, so don't enter the panning drag mode.
                    if allow_x || allow_y {
                        doc.drag_mode = DragMode::Panning(PanState {
                            target,
                            last_x: event.screen_x(),
                            last_y: event.screen_y(),
                            allow_x,
                            allow_y,
                            tracker: VelocityTracker::new(Instant::now()),
                        });
                    }
                }
            }
        }
    }

    if let DragMode::Panning(state) = &mut doc.drag_mode {
        let target = state.target;
        let (dx, dy) = state.update(Instant::now(), event.screen_x(), event.screen_y());

        let has_changed = doc.scroll_chain_by(Some(target), dx, dy, &mut dispatch_event);
        return has_changed;
    }

    if let DragMode::ScrollbarDrag(state) = &mut doc.drag_mode {
        let ScrollbarRef { node_id, axis } = state.scrollbar;
        let pos = match axis {
            AbsoluteAxis::Horizontal => x,
            AbsoluteAxis::Vertical => y,
        };
        let delta_px = (pos - state.last_pos) as f64;
        state.last_pos = pos;

        // Thumb px -> content px. scroll_by uses wheel-delta semantics
        // (positive delta decreases the offset), so negate.
        let ratio = doc.nodes[node_id].scrollbar_drag_ratio(axis);
        let (dx, dy) = match axis {
            AbsoluteAxis::Horizontal => (-delta_px * ratio, 0.0),
            AbsoluteAxis::Vertical => (0.0, -delta_px * ratio),
        };
        let has_changed = doc.scroll_chain_by(Some(node_id), dx, dy, &mut dispatch_event);
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

    // `target` is the event's canonicalized target (never a layout-generated
    // node), while the hit may be an anonymous block (e.g. bare text wrapped
    // in an anonymous box). Compare against the hit's canonical DOM ancestor
    // so selection drags keep working over anonymous blocks.
    if doc.nearest_non_anonymous_ancestor(hit.node_id) != Some(target) {
        return changed;
    }

    let node = &mut doc.nodes[target];
    let Some(el) = node.data.downcast_element_mut() else {
        // Handle text selection extension for non-element nodes
        if buttons != MouseEventButtons::None
            && doc.drag_mode == DragMode::Selecting
            && doc.extend_text_selection_to_point(x, y)
        {
            changed = true;
        }
        return changed;
    };

    let disabled = el.attr(local_name!("disabled")).is_some();
    if disabled {
        return changed;
    }

    let final_layout = el.layout_data().final_layout;
    if let SpecialElementData::TextInput(ref mut text_input_data) = el.special_data {
        if buttons == MouseEventButtons::None {
            return changed;
        }

        let mut content_box_offset = taffy::Point {
            x: final_layout.padding.left + final_layout.border.left,
            y: final_layout.padding.top + final_layout.border.top,
        };
        if !text_input_data.is_multiline {
            let layout = text_input_data.editor.try_layout().unwrap();
            let content_box_height = final_layout.content_box_height();
            let input_height = layout.height() / layout.scale();
            let y_offset = ((content_box_height - input_height) / 2.0).max(0.0);

            content_box_offset.y += y_offset;
        }

        // Account for the input's scroll offset (stored in CSS pixels, scaled here to device
        // pixels) when mapping the pointer location into the text content's coordinate space.
        let scroll_offset = text_input_data.scroll_offset as f64 * doc.viewport.scale_f64();
        let (scroll_x, scroll_y) = if text_input_data.is_multiline {
            (0.0, scroll_offset)
        } else {
            (scroll_offset, 0.0)
        };

        let x = (hit.x - content_box_offset.x) as f64 * doc.viewport.scale_f64() + scroll_x;
        let y = (hit.y - content_box_offset.y) as f64 * doc.viewport.scale_f64() + scroll_y;

        text_input_data
            .editor
            .driver(&mut doc.font_ctx.lock().unwrap(), &mut doc.layout_ctx)
            .extend_selection_to_point(x as f32, y as f32);

        changed = true;
    } else if event.is_mouse()
        && buttons != MouseEventButtons::None
        && doc.drag_mode == DragMode::Selecting
        && doc.extend_text_selection_to_point(x, y)
    {
        changed = true;
    }

    changed
}

pub(crate) fn handle_pointerdown(
    doc: &mut BaseDocument,
    _target: NodeId,
    x: f32,
    y: f32,
    button: MouseEventButton,
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
    let now = Instant::now();
    doc.carried_fling = match &doc.scroll_animation {
        ScrollAnimationState::Fling(fling) => Some(CarriedFling {
            time: now,
            velocity: fling.velocity_at(now),
        }),
        _ => None,
    };
    doc.scroll_animation = ScrollAnimationState::None;

    let Some(hit) = doc.hit(x, y) else {
        // Clear text selection when clicking outside any element
        doc.clear_text_selection();
        return;
    };

    // Scrollbar thumb drags take precedence over content interactions and
    // are not dispatched to the page (matching native scrollbars). A
    // faded-out thumb doesn't capture: the click goes to the content.
    if button == MouseEventButton::Main {
        if let (_, Some(scrollbar)) = doc.hit_with_scrollbar(x, y)
            && doc.scrollbar_opacity(scrollbar.node_id) > 0.0
        {
            doc.drag_mode = DragMode::ScrollbarDrag(ScrollbarDragState {
                scrollbar,
                last_pos: match scrollbar.axis {
                    AbsoluteAxis::Horizontal => x,
                    AbsoluteAxis::Vertical => y,
                },
            });
            doc.shell_provider.request_redraw();
            return;
        }
    }

    // Use hit.node_id for determining the actual clicked element.
    // This may differ from `target` for anonymous blocks (which are layout children
    // but not DOM children), so we use the hit result for text selection.
    let actual_target = hit.node_id;

    // Check what kind of element we're dealing with and extract needed info
    enum ClickTarget {
        TextInput {
            content_box_offset: taffy::Point<f32>,
            /// Scroll offset of the input along each axis, in scaled (device) pixels.
            scroll: taffy::Point<f64>,
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
                        x: node.final_layout().padding.left + node.final_layout().border.left,
                        y: node.final_layout().padding.top + node.final_layout().border.top,
                    };
                    if !text_input_data.is_multiline {
                        let layout = text_input_data.editor.try_layout().unwrap();
                        let content_box_height = node.final_layout().content_box_height();
                        let input_height = layout.height() / layout.scale();
                        let y_offset = ((content_box_height - input_height) / 2.0).max(0.0);
                        content_box_offset.y += y_offset;
                    }
                    // `scroll_offset` is stored in CSS pixels; scale it to device pixels to
                    // match the editor's coordinate space.
                    let scroll_offset =
                        text_input_data.scroll_offset as f64 * doc.viewport.scale_f64();
                    let scroll = if text_input_data.is_multiline {
                        taffy::Point {
                            x: 0.0,
                            y: scroll_offset,
                        }
                    } else {
                        taffy::Point {
                            x: scroll_offset,
                            y: 0.0,
                        }
                    };
                    ClickTarget::TextInput {
                        content_box_offset,
                        scroll,
                    }
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
        ClickTarget::TextInput {
            content_box_offset,
            scroll,
        } => {
            // Clear general text selection when focusing a text input
            doc.clear_text_selection();

            let tx = (hit.x - content_box_offset.x) as f64 * doc.viewport.scale_f64() + scroll.x;
            let ty = (hit.y - content_box_offset.y) as f64 * doc.viewport.scale_f64() + scroll.y;

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

pub(crate) fn handle_pointerup<F: FnMut(DomEvent)>(
    doc: &mut BaseDocument,
    target: NodeId,
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

    // Repaint so a dragged scrollbar thumb drops its active styling, and
    // restart its fade-out delay now that the drag no longer holds it shown
    if let DragMode::ScrollbarDrag(state) = &drag_mode {
        doc.show_scrollbars(state.scrollbar.node_id);
        doc.shell_provider.request_redraw();
    }

    let carried = doc.carried_fling.take();
    if let DragMode::Panning(state) = &drag_mode {
        if let Some(fling) = state.generate_fling(Instant::now(), carried.as_ref()) {
            doc.scroll_animation = ScrollAnimationState::Fling(fling);
            doc.shell_provider.request_redraw();
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
    target: NodeId,
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
                    let mut is_checked = false;
                    doc.snapshot_node_and(node_id, ElementState::CHECKED, |node| {
                        if let Some(el) = node.element_data_mut() {
                            is_checked = BaseDocument::toggle_checkbox(el);
                        }
                        node.mark_ancestors_dirty();
                    });
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
                    if let Some(radio_set) = el.attr(local_name!("name")).map(str::to_string) {
                        BaseDocument::toggle_radio(doc, radio_set, node_id);
                    } else if el.checkbox_input_checked().is_some() {
                        doc.snapshot_node_and(node_id, ElementState::CHECKED, |node| {
                            if let Some(el) = node.element_data_mut() {
                                el.set_checkbox_input_checked(true);
                            }
                            node.mark_ancestors_dirty();
                        });
                    }

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
                // Activating the first <summary> of a <details> element toggles
                // the details' `open` attribute (expand/collapse).
                local_name!("summary") => {
                    if let Some(parent_id) = doc.nodes[node_id].parent {
                        let parent = &doc.nodes[parent_id];
                        let is_first_summary = parent
                            .data
                            .is_element_with_tag_name(&local_name!("details"))
                            && parent.children.iter().copied().find(|&child_id| {
                                doc.nodes[child_id]
                                    .data
                                    .is_element_with_tag_name(&local_name!("summary"))
                            }) == Some(node_id);

                        if is_first_summary {
                            doc.toggle_details_open(parent_id);
                            generate_focus_events(
                                doc,
                                &mut |doc| {
                                    doc.set_focus_to(node_id);
                                },
                                dispatch_event,
                            );
                            break 'matched true;
                        }
                    }
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
                    if let Some(href) = el.attr(local_name!("href")).map(str::to_string) {
                        if let Some(url) = doc.url.resolve_relative(&href) {
                            // If the link only differs from the current document URL by its
                            // fragment (this includes links whose href is just `#fragment`),
                            // perform in-page fragment navigation (scrolling) instead of a
                            // full navigation.
                            if url.fragment().is_some() && doc.url.is_same_document(&url) {
                                doc.scroll_to_fragment(url.fragment().unwrap_or_default());
                            } else {
                                doc.navigation_provider.navigate_to(NavigationOptions::new(
                                    url,
                                    None,
                                    doc.id(),
                                ));
                            }
                        } else {
                            #[cfg(feature = "tracing")]
                            tracing::warn!("{href} is not parseable as a url. : {:?}", *doc.url);
                        }
                        break 'matched true;
                    } else {
                        #[cfg(feature = "tracing")]
                        tracing::info!("Clicked link without href: {:?}", el.attrs());
                    }
                }
                local_name!("input") | local_name!("button")
                    if el.is_submit_button() || el.attr(local_name!("type")) == Some("submit") =>
                {
                    if let Some(form_owner) = doc.controls_to_form.get(&node_id) {
                        doc.submit_form(*form_owner, node_id);
                    }
                }
                #[cfg(feature = "file-input")]
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
    _: NodeId,
    event: BlitzWheelEvent,
    mut dispatch_event: F,
) {
    let (scroll_x, scroll_y) = match event.delta {
        BlitzWheelDelta::Lines(x, y) => (x * 20.0, y * 20.0),
        BlitzWheelDelta::Pixels(x, y) => (x, y),
    };

    let has_changed = doc.scroll_chain_by(
        doc.get_hover_node_id(),
        scroll_x,
        scroll_y,
        &mut dispatch_event,
    );
    if has_changed {
        doc.shell_provider.request_redraw();
    }
}

#[cfg(test)]
mod fling_tests {
    use super::*;

    #[test]
    fn fling_threshold_axis_mask_and_boost() {
        let now = Instant::now();
        let mut pan = PanState {
            target: NodeId::default(),
            last_x: 0.0,
            last_y: 0.0,
            allow_x: true,
            allow_y: false,
            tracker: VelocityTracker::new(now),
        };
        assert!(pan.generate_fling(now, None).is_none());
        let release = now + Duration::from_millis(10);
        pan.update(release, 0.1, 100.0);
        assert!(pan.generate_fling(release, None).is_none());
        pan.update(now + Duration::from_millis(20), 200.0, 200.0);
        let release = now + Duration::from_millis(20);
        let carried = CarriedFling {
            time: now,
            velocity: Point { x: 1000.0, y: 0.0 },
        };
        let fling = pan.generate_fling(release, Some(&carried)).unwrap();
        assert!((fling.velocity_at(release).x - 8000.0).abs() < 1e-8);
        assert_eq!(fling.velocity_at(release).y, 0.0);
        assert!(
            pan.generate_fling(release + Duration::from_millis(41), Some(&carried))
                .is_none()
        );
    }
}
