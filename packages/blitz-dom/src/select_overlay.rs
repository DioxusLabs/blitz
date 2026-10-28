use crate::{
    BaseDocument, BoundingRect, NodeId, local_name,
    node::{SelectData, SpecialElementData, TextBrush},
};
use blitz_traits::events::{BlitzInputEvent, BlitzWheelDelta, DomEvent, DomEventData, UiEvent};
use keyboard_types::{Key, Modifiers, NamedKey};
use parley::Layout;

pub struct SelectOverlayRow {
    pub option: NodeId,
    pub rect: BoundingRect,
    pub disabled: bool,
    pub highlighted: bool,
    pub layout: Layout<TextBrush>,
}

pub struct SelectOverlay {
    pub select: NodeId,
    pub rect: BoundingRect,
    pub rows: Vec<SelectOverlayRow>,
    pub visible_rows: usize,
}

fn contains(rect: &BoundingRect, x: f32, y: f32) -> bool {
    x as f64 >= rect.x
        && y as f64 >= rect.y
        && (x as f64) < rect.x + rect.width
        && (y as f64) < rect.y + rect.height
}

impl BaseDocument {
    fn select_data_mut(&mut self, id: NodeId) -> Option<&mut SelectData> {
        match &mut self.nodes.get_mut(id)?.element_data_mut()?.special_data {
            SpecialElementData::Select(data) => Some(data),
            _ => None,
        }
    }

    pub fn close_select(&mut self) {
        if let Some(id) = self.open_select.take() {
            if let Some(data) = self.select_data_mut(id) {
                data.open = false;
            }
            self.changed_nodes.insert(id);
            self.shell_provider.request_redraw();
        }
    }

    pub fn open_select(&mut self, id: NodeId) -> bool {
        if !self.select_nodes.contains(&id)
            || self.nodes[id].attr(local_name!("disabled")).is_some()
            || !matches!(
                self.nodes[id].element_data().map(|e| &e.special_data),
                Some(SpecialElementData::Select(_))
            )
        {
            return false;
        }
        self.close_select();
        self.set_focus_to(id);
        let selected = self
            .selected_option(id)
            .filter(|&o| !self.option_disabled(o))
            .or_else(|| {
                self.select_options(id)
                    .into_iter()
                    .find(|&o| !self.option_disabled(o))
            });
        let data = self.select_data_mut(id).unwrap();
        data.open = true;
        data.highlighted = selected;
        data.scroll_offset = 0;
        self.open_select = Some(id);
        self.changed_nodes.insert(id);
        self.reveal_select_highlight(id);
        self.shell_provider.request_redraw();
        true
    }

    fn select_anchor(&self, id: NodeId) -> BoundingRect {
        let scale = self.viewport.scale_f64();
        let mut chain = Vec::new();
        let mut current = Some(id);
        while let Some(id) = current {
            chain.push(id);
            current = self.nodes[id].containing_block();
        }
        let mut matrix = kurbo::Affine::translate((
            -self.viewport_scroll().x * scale,
            -self.viewport_scroll().y * scale,
        ));
        let chain: Vec<_> = chain.into_iter().rev().collect();
        for (index, &current_id) in chain.iter().enumerate() {
            let node = &self.nodes[current_id];
            let layout = node.final_layout();
            let scroll = node.scroll_offset();
            if index > 0 {
                let parent = &self.nodes[chain[index - 1]];
                if node.taffy_position() == taffy::Position::Fixed
                    && parent.containing_block().is_none()
                {
                    matrix *= kurbo::Affine::translate((
                        (parent.scroll_offset().x + self.viewport_scroll().x) * scale,
                        (parent.scroll_offset().y + self.viewport_scroll().y) * scale,
                    ));
                }
                if parent.flags.is_inline_root() && !node.taffy_position().is_out_of_flow() {
                    let layout = parent.final_layout();
                    matrix *= kurbo::Affine::translate((
                        (layout.padding.left + layout.border.left) as f64 * scale,
                        (layout.padding.top + layout.border.top) as f64 * scale,
                    ));
                }
            }
            matrix *= kurbo::Affine::translate((
                (layout.location.x as f64 - scroll.x) * scale,
                (layout.location.y as f64 - scroll.y) * scale,
            ));
            if let Some(transform) = node.transform().as_deref() {
                matrix *= *transform;
            }
            if current_id == id {
                matrix *= kurbo::Affine::translate((scroll.x * scale, scroll.y * scale));
            }
        }
        let size = self.nodes[id].final_layout().size;
        let rect = matrix.transform_rect_bbox(kurbo::Rect::new(
            0.0,
            0.0,
            size.width as f64 * scale,
            size.height as f64 * scale,
        ));
        BoundingRect {
            x: rect.x0 / scale,
            y: rect.y0 / scale,
            width: rect.width() / scale,
            height: rect.height() / scale,
        }
    }

    pub fn select_overlay(&mut self) -> Option<SelectOverlay> {
        let id = self.open_select?;
        let node = self.get_node(id)?;
        let style = (*node.primary_styles()?).clone();
        let SpecialElementData::Select(data) = &node.element_data()?.special_data else {
            return None;
        };
        let highlight = data.highlighted;
        let offset = data.scroll_offset;
        let anchor = self.select_anchor(id);
        let font_size = style.get_font().slow_clone_font_size().used_size().px() as f64;
        let text_style = crate::stylo_to_parley::style(id, &style);
        let mut layouts = Vec::new();
        let mut width = anchor.width;
        let mut row_height = font_size * 1.2 + 8.0;
        for option in self.select_options(id) {
            let label = self.option_label(option);
            let mut layout = Layout::default();
            let mut fonts = self.font_ctx.lock().unwrap();
            let mut builder = self
                .layout_ctx
                .tree_builder(&mut fonts, 1.0, true, &text_style);
            builder.push_text(&label);
            builder.build_into(&mut layout);
            layout.break_all_lines(None);
            width = width.max(layout.width() as f64 + 24.0);
            row_height = row_height.max(layout.height() as f64 + 8.0);
            layouts.push((option, layout));
        }
        let viewport_width = self.viewport.window_size.0 as f64 / self.viewport.scale_f64();
        let viewport_height = self.viewport.window_size.1 as f64 / self.viewport.scale_f64();
        let below = (viewport_height - anchor.y - anchor.height).max(0.0);
        let above = anchor.y.max(0.0).min(viewport_height);
        let desired_height = layouts.len().min(20) as f64 * row_height + 2.0;
        let flip = below < desired_height && above > below;
        let space = if flip { above } else { below };
        let visible_rows = layouts
            .len()
            .min(20)
            .min(((space - 2.0).max(0.0) / row_height) as usize);
        let height = visible_rows as f64 * row_height + 2.0;
        width = width.min(viewport_width);
        let rect = BoundingRect {
            x: anchor.x.clamp(0.0, (viewport_width - width).max(0.0)),
            y: if flip {
                anchor.y - height
            } else {
                anchor.y + anchor.height
            },
            width,
            height,
        };
        let offset = offset.min(layouts.len().saturating_sub(visible_rows));
        let rows = layouts
            .into_iter()
            .skip(offset)
            .take(visible_rows)
            .enumerate()
            .map(|(index, (option, layout))| SelectOverlayRow {
                option,
                disabled: self.option_disabled(option),
                highlighted: highlight == Some(option),
                layout,
                rect: BoundingRect {
                    x: rect.x + 1.0,
                    y: rect.y + 1.0 + index as f64 * row_height,
                    width: (width - 2.0).max(0.0),
                    height: row_height,
                },
            })
            .collect();
        Some(SelectOverlay {
            select: id,
            rect,
            rows,
            visible_rows,
        })
    }

    fn reveal_select_highlight(&mut self, id: NodeId) {
        let Some(overlay) = self.select_overlay() else {
            return;
        };
        let options = self.select_options(id);
        let data = self.select_data_mut(id).unwrap();
        if let Some(index) = data
            .highlighted
            .and_then(|o| options.iter().position(|&id| id == o))
        {
            if index < data.scroll_offset {
                data.scroll_offset = index;
            }
            if index >= data.scroll_offset + overlay.visible_rows {
                data.scroll_offset = (index + 1).saturating_sub(overlay.visible_rows.max(1));
            }
        }
    }

    fn commit_select(&mut self, id: NodeId, option: NodeId, events: &mut Vec<DomEvent>) {
        if !self.option_disabled(option) && self.selected_option(id) != Some(option) {
            self.set_selected_option(id, Some(option));
            let value = self.option_value(option);
            events.push(DomEvent::new(
                id,
                DomEventData::Input(BlitzInputEvent {
                    value: value.clone(),
                }),
            ));
            events.push(DomEvent::new(
                id,
                DomEventData::Change(BlitzInputEvent { value }),
            ));
            self.shell_provider.request_redraw();
        }
    }

    pub(crate) fn handle_select_ui_event(
        &mut self,
        event: &UiEvent,
        events: &mut Vec<DomEvent>,
    ) -> bool {
        if self.select_pointer_capture
            && matches!(event, UiEvent::PointerUp(_) | UiEvent::PointerCancel(_))
        {
            self.select_pointer_capture = false;
            return true;
        }
        if let UiEvent::PointerDown(pointer) = event {
            if pointer.button != blitz_traits::events::MouseEventButton::Main {
                return false;
            }
            if let Some(id) = self.open_select {
                if let Some(overlay) = self.select_overlay() {
                    if let Some(row) = overlay
                        .rows
                        .iter()
                        .find(|r| contains(&r.rect, pointer.client_x(), pointer.client_y()))
                    {
                        if row.disabled {
                            self.select_pointer_capture = true;
                            return true;
                        }
                        self.commit_select(id, row.option, events);
                    }
                }
                self.close_select();
                self.select_pointer_capture = true;
                return true;
            }
            if let Some(hit) = self.hit(pointer.page_x(), pointer.page_y()) {
                let mut current = Some(hit.node_id);
                while let Some(id) = current {
                    if self.select_nodes.contains(&id) {
                        if self.nodes[id].attr(local_name!("disabled")).is_none()
                            && matches!(
                                self.nodes[id].element_data().map(|e| &e.special_data),
                                Some(SpecialElementData::Select(_))
                            )
                        {
                            crate::events::generate_focus_events(
                                self,
                                &mut |doc| {
                                    doc.set_focus_to(id);
                                },
                                &mut |event| events.push(event),
                            );
                            self.open_select(id);
                            self.select_pointer_capture = true;
                            return true;
                        }
                        break;
                    }
                    current = self.nodes[id].parent;
                }
            }
        }
        if let Some(id) = self.open_select {
            match event {
                UiEvent::PointerMove(pointer) => {
                    if let Some(overlay) = self.select_overlay() {
                        let option = overlay
                            .rows
                            .iter()
                            .find(|r| {
                                !r.disabled
                                    && contains(&r.rect, pointer.client_x(), pointer.client_y())
                            })
                            .map(|r| r.option);
                        if let Some(option) = option {
                            let data = self.select_data_mut(id).unwrap();
                            if data.highlighted != Some(option) {
                                data.highlighted = Some(option);
                                self.shell_provider.request_redraw();
                            }
                        }
                    }
                    return true;
                }
                UiEvent::Wheel(wheel) => {
                    if let Some(overlay) = self.select_overlay() {
                        if contains(&overlay.rect, wheel.client_x(), wheel.client_y()) {
                            let delta = match wheel.delta {
                                BlitzWheelDelta::Lines(_, y) => y,
                                BlitzWheelDelta::Pixels(_, y) => y / 24.0,
                            };
                            let max = self
                                .select_options(id)
                                .len()
                                .saturating_sub(overlay.visible_rows);
                            let data = self.select_data_mut(id).unwrap();
                            data.scroll_offset = (data.scroll_offset as f64 - delta)
                                .round()
                                .clamp(0.0, max as f64)
                                as usize;
                            self.shell_provider.request_redraw();
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub(crate) fn handle_select_key(
        &mut self,
        event: &blitz_traits::events::BlitzKeyEvent,
        events: &mut Vec<DomEvent>,
    ) -> bool {
        let Some(id) = self
            .focus_node_id
            .filter(|id| self.select_nodes.contains(id))
        else {
            return false;
        };
        if self.nodes[id].attr(local_name!("disabled")).is_some()
            || !event.state.is_pressed()
            || !matches!(
                self.nodes[id].element_data().map(|e| &e.special_data),
                Some(SpecialElementData::Select(_))
            )
        {
            return false;
        }
        let open = self.open_select == Some(id);
        if event.key == Key::Named(NamedKey::Enter)
            || matches!(&event.key, Key::Character(s) if s == " ")
        {
            if open {
                if let Some(option) = self.select_data_mut(id).and_then(|d| d.highlighted) {
                    self.commit_select(id, option, events);
                }
                self.close_select();
            } else {
                self.open_select(id);
            }
            return true;
        }
        match &event.key {
            Key::Named(NamedKey::Tab) => {
                self.close_select();
                return false;
            }
            Key::Named(NamedKey::Escape) if open => {
                self.close_select();
                return true;
            }
            Key::Named(NamedKey::ArrowDown) if event.modifiers.contains(Modifiers::ALT) => {
                self.open_select(id);
                return true;
            }
            _ => {}
        }
        let options: Vec<_> = self
            .select_options(id)
            .into_iter()
            .filter(|&o| !self.option_disabled(o))
            .collect();
        let current = if open {
            self.select_data_mut(id).and_then(|d| d.highlighted)
        } else {
            self.selected_option(id)
        };
        let index = current.and_then(|o| options.iter().position(|&id| id == o));
        let next = match &event.key {
            Key::Named(NamedKey::ArrowDown) => options
                .get(index.map_or(0, |i| (i + 1).min(options.len().saturating_sub(1))))
                .copied(),
            Key::Named(NamedKey::ArrowUp) => options
                .get(index.map_or(options.len().saturating_sub(1), |i| i.saturating_sub(1)))
                .copied(),
            Key::Named(NamedKey::Home) => options.first().copied(),
            Key::Named(NamedKey::End) => options.last().copied(),
            Key::Character(text)
                if !event
                    .modifiers
                    .intersects(Modifiers::CONTROL | Modifiers::META | Modifiers::ALT) =>
            {
                let start = index.map_or(0, |i| i + 1);
                options
                    .iter()
                    .cycle()
                    .skip(start)
                    .take(options.len())
                    .find(|&&o| {
                        self.option_label(o)
                            .to_lowercase()
                            .starts_with(&text.to_lowercase())
                    })
                    .copied()
            }
            _ => return false,
        };
        if let Some(option) = next {
            if open {
                self.select_data_mut(id).unwrap().highlighted = Some(option);
                self.reveal_select_highlight(id);
                self.shell_provider.request_redraw();
            } else {
                self.commit_select(id, option, events);
            }
        }
        true
    }
}
