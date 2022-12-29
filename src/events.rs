use keyboard_types::Code;
use piet_wgpu::kurbo::Point;
use std::{
    any::Any,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use taffy::prelude::Size;
use tao::event::MouseButton;

use dioxus::{
    core::{ElementId, Mutations},
    events::{KeyboardData, MouseData},
    prelude::dioxus_elements::{
        geometry::{
            euclid::Point2D, ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint,
        },
        input_data::{self, keyboard_types::Modifiers, MouseButtonSet},
    },
};
use dioxus_native_core::tree::TreeView;

use tao::keyboard::Key;

use crate::{focus::FocusState, mouse::get_hovered, node::PreventDefault, Dom, TaoEvent};

const DBL_CLICK_TIME: Duration = Duration::from_millis(500);

struct CursorState {
    position: Coordinates,
    buttons: MouseButtonSet,
    last_click: Option<Instant>,
    last_pressed_element: Option<ElementId>,
    last_clicked_element: Option<ElementId>,
    hovered: Option<ElementId>,
}

impl CursorState {
    fn get_event_mouse_data(&self) -> MouseData {
        // MouseData::new(coordinates, trigger_button, held_buttons, modifiers)
        MouseData::new(
            Coordinates::new(
                self.position.screen(),
                self.position.client(),
                self.position.element(),
                self.position.page(),
            ),
            None,
            self.buttons,
            Modifiers::default(),
        )
    }
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            position: Coordinates::new(
                Point2D::default(),
                Point2D::default(),
                Point2D::default(),
                Point2D::default(),
            ),
            buttons: Default::default(),
            last_click: Default::default(),
            last_pressed_element: Default::default(),
            last_clicked_element: Default::default(),
            hovered: Default::default(),
        }
    }
}

#[derive(Default)]
struct EventState {
    modifier_state: Modifiers,
    cursor_state: CursorState,
    focus_state: Arc<Mutex<FocusState>>,
}

pub struct AnyEvent {
    pub name: &'static str,
    pub data: Box<dyn Any + Send + Sync>,
    pub element: ElementId,
    pub bubbles: bool,
}

#[derive(Default)]
pub struct BlitzEventHandler {
    state: EventState,
    queued_events: Vec<AnyEvent>,
}

impl BlitzEventHandler {
    pub(crate) fn new(focus_state: Arc<Mutex<FocusState>>) -> Self {
        Self {
            state: EventState {
                focus_state,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub(crate) fn register_event(
        &mut self,
        event: &TaoEvent,
        rdom: &mut Dom,
        viewport_size: &Size<u32>,
    ) {
        match event {
            tao::event::Event::NewEvents(_) => (),
            tao::event::Event::WindowEvent {
                window_id: _,
                event,
                ..
            } => {
                match event {
                    tao::event::WindowEvent::Resized(_) => (),
                    tao::event::WindowEvent::Moved(_) => (),
                    tao::event::WindowEvent::CloseRequested => (),
                    tao::event::WindowEvent::Destroyed => (),
                    tao::event::WindowEvent::DroppedFile(_) => (),
                    tao::event::WindowEvent::HoveredFile(_) => (),
                    tao::event::WindowEvent::HoveredFileCancelled => (),
                    tao::event::WindowEvent::ReceivedImeText(_) => (),
                    tao::event::WindowEvent::Focused(_) => (),
                    tao::event::WindowEvent::KeyboardInput {
                        device_id: _,
                        event,
                        is_synthetic: _,
                        ..
                    } => {
                        let key = map_key(&event.logical_key);
                        let code = map_code(&event.physical_key);

                        let data = KeyboardData::new(
                            key,
                            code,
                            match event.location {
                                tao::keyboard::KeyLocation::Standard => {
                                    input_data::keyboard_types::Location::Standard
                                }
                                tao::keyboard::KeyLocation::Left => {
                                    input_data::keyboard_types::Location::Left
                                }
                                tao::keyboard::KeyLocation::Right => {
                                    input_data::keyboard_types::Location::Right
                                }
                                tao::keyboard::KeyLocation::Numpad => {
                                    input_data::keyboard_types::Location::Numpad
                                }
                                _ => todo!(),
                            },
                            event.repeat,
                            self.state.modifier_state,
                        );

                        // keypress events are only triggered when a key that has text is pressed
                        if let tao::event::ElementState::Pressed = event.state {
                            if event.text.is_some() {
                                self.queued_events.push(AnyEvent {
                                    name: "keypress",
                                    element: ElementId(1),
                                    data: Box::new(data.clone()),
                                    bubbles: true,
                                });
                            }
                            if let Key::Tab = event.logical_key {
                                self.state.focus_state.lock().unwrap().progress(
                                    rdom,
                                    !self.state.modifier_state.contains(Modifiers::SHIFT),
                                );
                                return;
                            }
                        }

                        if let Some(element) = self.state.focus_state.lock().ok().and_then(|lock| {
                            lock.last_focused_id
                                .and_then(|last| rdom[last].mounted_id())
                        }) {
                            self.queued_events.push(AnyEvent {
                                element,
                                name: match event.state {
                                    tao::event::ElementState::Pressed => "keydown",
                                    tao::event::ElementState::Released => "keyup",
                                    _ => todo!(),
                                },
                                data: Box::new(data),
                                bubbles: true,
                            });
                        }
                    }
                    tao::event::WindowEvent::ModifiersChanged(mods) => {
                        let mut modifiers = Modifiers::empty();
                        if mods.alt_key() {
                            modifiers |= Modifiers::ALT;
                        }
                        if mods.control_key() {
                            modifiers |= Modifiers::CONTROL;
                        }
                        if mods.super_key() {
                            modifiers |= Modifiers::META;
                        }
                        if mods.shift_key() {
                            modifiers |= Modifiers::SHIFT;
                        }
                        self.state.modifier_state = modifiers;
                    }
                    tao::event::WindowEvent::CursorMoved {
                        device_id: _,
                        position,
                        ..
                    } => {
                        let pos = Point::new(position.x, position.y);
                        let hovered = get_hovered(rdom, viewport_size, pos);
                        let (mouse_x, mouse_y) = (pos.x as i32, pos.y as i32);
                        let screen_point = ScreenPoint::new(mouse_x as f64, mouse_y as f64);
                        let client_point = ClientPoint::new(mouse_x as f64, mouse_y as f64);
                        let page_point = PagePoint::new(mouse_x as f64, mouse_y as f64);
                        // the position of the element is subtracted later
                        let element_point = ElementPoint::new(mouse_x as f64, mouse_y as f64);
                        let position =
                            Coordinates::new(screen_point, client_point, element_point, page_point);

                        let data = MouseData::new(
                            Coordinates::new(screen_point, client_point, element_point, page_point),
                            None,
                            self.state.cursor_state.buttons,
                            self.state.modifier_state,
                        );
                        match (hovered, self.state.cursor_state.hovered) {
                            (Some(hovered), Some(old_hovered)) => {
                                if hovered != old_hovered {
                                    self.queued_events.push(AnyEvent {
                                        element: hovered,
                                        name: "mouseenter",
                                        data: Box::new(data.clone()),
                                        bubbles: true,
                                    });
                                    self.queued_events.push(AnyEvent {
                                        element: old_hovered,
                                        name: "mouseleave",
                                        data: Box::new(data),
                                        bubbles: true,
                                    });
                                    self.state.cursor_state.hovered = Some(hovered);
                                }
                            }
                            (Some(hovered), None) => {
                                self.queued_events.push(AnyEvent {
                                    element: hovered,
                                    name: "mouseenter",
                                    data: Box::new(data),
                                    bubbles: true,
                                });
                                self.state.cursor_state.hovered = Some(hovered);
                            }
                            (None, Some(old_hovered)) => {
                                self.queued_events.push(AnyEvent {
                                    element: old_hovered,
                                    name: "mouseleave",
                                    data: Box::new(data),
                                    bubbles: true,
                                });
                                self.state.cursor_state.hovered = None;
                            }
                            (None, None) => (),
                        }
                        self.state.cursor_state.position = position;
                    }
                    tao::event::WindowEvent::CursorEntered { device_id: _ } => {}
                    tao::event::WindowEvent::CursorLeft { device_id: _ } => {
                        if let Some(old_hovered) = self.state.cursor_state.hovered {
                            self.queued_events.push(AnyEvent {
                                element: old_hovered,
                                name: "mouseleave",
                                data: Box::new(self.state.cursor_state.get_event_mouse_data()),
                                bubbles: true,
                            });
                            self.state.cursor_state.hovered = None;
                        }
                    }
                    tao::event::WindowEvent::MouseWheel {
                        device_id: _,
                        delta: _,
                        phase: _,
                        ..
                    } => (),
                    tao::event::WindowEvent::MouseInput {
                        device_id: _,
                        state,
                        button,
                        ..
                    } => {
                        if let Some(hovered) = self.state.cursor_state.hovered {
                            let button = match button {
                                MouseButton::Left => input_data::MouseButton::Primary,
                                MouseButton::Middle => input_data::MouseButton::Auxiliary,
                                MouseButton::Right => input_data::MouseButton::Secondary,
                                MouseButton::Other(num) => match num {
                                    4 => input_data::MouseButton::Fourth,
                                    5 => input_data::MouseButton::Fifth,
                                    _ => input_data::MouseButton::Unknown,
                                },
                                _ => input_data::MouseButton::Unknown,
                            };

                            match state {
                                tao::event::ElementState::Pressed => {
                                    self.state.cursor_state.buttons |= button;
                                }
                                tao::event::ElementState::Released => {
                                    self.state.cursor_state.buttons.remove(button);
                                }
                                _ => todo!(),
                            }

                            let pos = &self.state.cursor_state.position;

                            let data = MouseData::new(
                                Coordinates::new(
                                    pos.screen(),
                                    pos.client(),
                                    pos.element(),
                                    pos.page(),
                                ),
                                None,
                                self.state.cursor_state.buttons,
                                self.state.modifier_state,
                            );

                            let prevent_default = &rdom[hovered].state.prevent_default;
                            match state {
                                tao::event::ElementState::Pressed => {
                                    self.queued_events.push(AnyEvent {
                                        element: hovered,
                                        name: "mousedown",
                                        data: Box::new(data),
                                        bubbles: true,
                                    });
                                    self.state.cursor_state.last_pressed_element = Some(hovered);
                                }
                                tao::event::ElementState::Released => {
                                    self.queued_events.push(AnyEvent {
                                        element: hovered,
                                        name: "mouseup",
                                        data: Box::new(data.clone()),
                                        bubbles: true,
                                    });

                                    // click events only trigger if the mouse button is pressed and released on the same element
                                    if self.state.cursor_state.last_pressed_element.take()
                                        == Some(hovered)
                                    {
                                        self.queued_events.push(AnyEvent {
                                            element: hovered,
                                            name: "click",
                                            data: Box::new(data.clone()),
                                            bubbles: true,
                                        });

                                        if let Some(last_clicked) =
                                            self.state.cursor_state.last_click.take()
                                        {
                                            if self.state.cursor_state.last_clicked_element
                                                == Some(hovered)
                                                && last_clicked.elapsed() < DBL_CLICK_TIME
                                            {
                                                self.queued_events.push(AnyEvent {
                                                    element: hovered,
                                                    name: "dblclick",
                                                    data: Box::new(data),
                                                    bubbles: true,
                                                });
                                            }
                                        }

                                        self.state.cursor_state.last_clicked_element =
                                            Some(hovered);
                                        self.state.cursor_state.last_click = Some(Instant::now());
                                    }
                                }
                                _ => todo!(),
                            }
                            if *prevent_default != PreventDefault::MouseUp
                                && rdom[hovered].state.focus.level.focusable()
                            {
                                self.state
                                    .focus_state
                                    .lock()
                                    .unwrap()
                                    .set_focus(rdom, rdom.element_to_node_id(hovered));
                            }
                        }
                    }
                    tao::event::WindowEvent::TouchpadPressure {
                        device_id: _,
                        pressure: _,
                        stage: _,
                    } => (),
                    tao::event::WindowEvent::AxisMotion {
                        device_id: _,
                        axis: _,
                        value: _,
                    } => (),
                    tao::event::WindowEvent::Touch(_) => (),
                    tao::event::WindowEvent::ScaleFactorChanged {
                        scale_factor: _,
                        new_inner_size: _,
                    } => (),
                    tao::event::WindowEvent::ThemeChanged(_) => (),
                    tao::event::WindowEvent::DecorationsClick => (),
                    _ => (),
                }
            }
            tao::event::Event::DeviceEvent {
                device_id: _,
                event: _,
                ..
            } => (),
            tao::event::Event::MenuEvent {
                window_id: _,
                menu_id: _,
                origin: _,
                ..
            } => (),
            tao::event::Event::TrayEvent {
                bounds: _,
                event: _,
                position: _,
                ..
            } => (),
            tao::event::Event::GlobalShortcutEvent(_) => (),
            tao::event::Event::Suspended => (),
            tao::event::Event::Resumed => (),
            tao::event::Event::MainEventsCleared => (),
            tao::event::Event::RedrawRequested(_) => (),
            tao::event::Event::RedrawEventsCleared => (),
            tao::event::Event::LoopDestroyed => (),
            _ => (),
        }
    }

    pub fn drain_events(&mut self) -> Vec<AnyEvent> {
        let mut events = Vec::new();
        std::mem::swap(&mut self.queued_events, &mut events);
        events
    }

    fn prune_id(&mut self, removed: ElementId) {
        if let Some(id) = self.state.cursor_state.hovered {
            if id == removed {
                self.state.cursor_state.hovered = None;
            }
        }
        if let Some(id) = self.state.cursor_state.last_pressed_element {
            if id == removed {
                self.state.cursor_state.last_pressed_element = None;
            }
        }
        if let Some(id) = self.state.cursor_state.last_clicked_element {
            if id == removed {
                self.state.cursor_state.last_clicked_element = None;
            }
        }
    }

    pub(crate) fn prune(&mut self, mutations: &Mutations, rdom: &Dom) {
        fn remove_children(handler: &mut BlitzEventHandler, rdom: &Dom, removed: ElementId) {
            handler.prune_id(removed);
            if let Some(children) = rdom.children(rdom.element_to_node_id(removed)) {
                for child in children {
                    if let Some(child_id) = child.mounted_id() {
                        remove_children(handler, rdom, child_id);
                    }
                }
            }
        }
        for m in &mutations.edits {
            match m {
                dioxus::core::Mutation::ReplaceWith { id, .. } => remove_children(self, rdom, *id),
                dioxus::core::Mutation::Remove { id } => remove_children(self, rdom, *id),
                _ => (),
            }
        }
    }

    pub(crate) fn clean(&self) -> bool {
        self.state.focus_state.lock().unwrap().clean()
    }
}

fn map_key(key: &tao::keyboard::Key) -> keyboard_types::Key {
    use tao::keyboard::Key::*;
    match key {
        Space => keyboard_types::Key::Character(" ".to_string()),
        _ => {
            let key = serde_json::to_value(key).unwrap();
            serde_json::from_value(key)
                .ok()
                .unwrap_or(keyboard_types::Key::Unidentified)
        }
    }
}

fn map_code(code: &tao::keyboard::KeyCode) -> keyboard_types::Code {
    use tao::keyboard::KeyCode::*;
    match code {
        SuperLeft | SuperRight => keyboard_types::Code::Super,
        _ => input_data::keyboard_types::Code::from_str(&code.to_string())
            .unwrap_or(Code::Unidentified),
    }
}
