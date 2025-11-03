use crate::BlitzShellProvider;
use crate::convert_events::{
    color_scheme_to_theme, theme_to_color_scheme, winit_ime_to_blitz, winit_key_event_to_blitz,
    winit_modifiers_to_kbt_modifiers,
};
use crate::event::{BlitzShellEvent, create_waker};
use anyrender::WindowRenderer;
use blitz_dom::Document;
use blitz_paint::paint_scene;
use blitz_traits::events::{BlitzMouseButtonEvent, MouseEventButton, MouseEventButtons, UiEvent};
use blitz_traits::shell::Viewport;
use winit::keyboard::PhysicalKey;

use std::sync::Arc;
use std::task::Waker;
use std::time::Instant;
use winit::event::{ElementState, MouseButton};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{Theme, WindowAttributes, WindowId};
use winit::{event::Modifiers, event::WindowEvent, keyboard::KeyCode, window::Window};

#[cfg(feature = "accessibility")]
use crate::accessibility::AccessibilityState;

pub struct WindowConfig<Rend: WindowRenderer> {
    doc: Box<dyn Document>,
    attributes: WindowAttributes,
    renderer: Rend,
}

impl<Rend: WindowRenderer> WindowConfig<Rend> {
    pub fn new(doc: Box<dyn Document>, renderer: Rend) -> Self {
        Self::with_attributes(doc, renderer, Window::default_attributes())
    }

    pub fn with_attributes(
        doc: Box<dyn Document>,
        renderer: Rend,
        attributes: WindowAttributes,
    ) -> Self {
        WindowConfig {
            doc,
            attributes,
            renderer,
        }
    }
}

pub struct View<Rend: WindowRenderer> {
    pub doc: Box<dyn Document>,

    pub renderer: Rend,
    pub waker: Option<Waker>,

    pub event_loop_proxy: EventLoopProxy<BlitzShellEvent>,
    pub window: Arc<Window>,

    /// The state of the keyboard modifiers (ctrl, shift, etc). Winit/Tao don't track these for us so we
    /// need to store them in order to have access to them when processing keypress events
    pub theme_override: Option<Theme>,
    pub keyboard_modifiers: Modifiers,
    pub buttons: MouseEventButtons,
    pub mouse_pos: (f32, f32),
    pub animation_timer: Option<Instant>,
    pub is_visible: bool,

    #[cfg(feature = "accessibility")]
    /// Accessibility adapter for `accesskit`.
    pub accessibility: AccessibilityState,
}

impl<Rend: WindowRenderer> View<Rend> {
    pub fn init(
        config: WindowConfig<Rend>,
        event_loop: &ActiveEventLoop,
        proxy: &EventLoopProxy<BlitzShellEvent>,
    ) -> Self {
        let winit_window = Arc::from(event_loop.create_window(config.attributes).unwrap());

        // Create viewport
        let size = winit_window.inner_size();
        let scale = winit_window.scale_factor() as f32;
        let theme = winit_window.theme().unwrap_or(Theme::Light);
        let color_scheme = theme_to_color_scheme(theme);
        let viewport = Viewport::new(size.width, size.height, scale, color_scheme);

        // Create shell provider
        let shell_provider = BlitzShellProvider::new(winit_window.clone());

        let mut doc = config.doc;
        doc.set_viewport(viewport);
        doc.set_shell_provider(Arc::new(shell_provider));

        // If the document title is set prior to the window being created then it will
        // have been sent to a dummy ShellProvider and won't get picked up.
        // So we look for it here and set it if present.
        let title = doc.find_title_node().map(|node| node.text_content());
        if let Some(title) = title {
            winit_window.set_title(&title);
        }

        Self {
            renderer: config.renderer,
            waker: None,
            animation_timer: None,
            keyboard_modifiers: Default::default(),
            event_loop_proxy: proxy.clone(),
            window: winit_window.clone(),
            doc,
            theme_override: None,
            buttons: MouseEventButtons::None,
            mouse_pos: Default::default(),
            is_visible: winit_window.is_visible().unwrap_or(true),
            #[cfg(feature = "accessibility")]
            accessibility: AccessibilityState::new(&winit_window, proxy.clone()),
        }
    }

    pub fn replace_document(&mut self, new_doc: Box<dyn Document>, retain_scroll_position: bool) {
        let scroll = self.doc.viewport_scroll();
        let viewport = self.doc.viewport().clone();
        let shell_provider = self.doc.shell_provider.clone();

        self.doc = new_doc;
        self.doc.set_viewport(viewport);
        self.doc.set_shell_provider(shell_provider);
        self.poll();
        self.request_redraw();

        if retain_scroll_position {
            self.doc.set_viewport_scroll(scroll);
        }
    }

    pub fn theme_override(&self) -> Option<Theme> {
        self.theme_override
    }

    pub fn current_theme(&self) -> Theme {
        color_scheme_to_theme(self.doc.viewport().color_scheme)
    }

    pub fn set_theme_override(&mut self, theme: Option<Theme>) {
        self.theme_override = theme;
        let theme = theme.or(self.window.theme()).unwrap_or(Theme::Light);
        self.with_viewport(|v| v.color_scheme = theme_to_color_scheme(theme));
    }

    pub fn downcast_doc_mut<T: 'static>(&mut self) -> &mut T {
        self.doc.as_any_mut().downcast_mut::<T>().unwrap()
    }

    pub fn current_animation_time(&mut self) -> f64 {
        match &self.animation_timer {
            Some(start) => Instant::now().duration_since(*start).as_secs_f64(),
            None => {
                self.animation_timer = Some(Instant::now());
                0.0
            }
        }
    }
}

impl<Rend: WindowRenderer> View<Rend> {
    pub fn resume(&mut self) {
        // Resolve dom
        let animation_time = self.current_animation_time();
        self.doc.resolve(animation_time);

        // Resume renderer
        let (width, height) = self.doc.viewport().window_size;
        let scale = self.doc.viewport().scale_f64();
        self.renderer.resume(self.window.clone(), width, height);
        if !self.renderer.is_active() {
            panic!("Renderer failed to resume");
        };

        // Render
        self.renderer
            .render(|scene| paint_scene(scene, &self.doc, scale, width, height));

        // Set waker
        self.waker = Some(create_waker(&self.event_loop_proxy, self.window_id()));
    }

    pub fn suspend(&mut self) {
        self.waker = None;
        self.renderer.suspend();
    }

    pub fn poll(&mut self) -> bool {
        if let Some(waker) = &self.waker {
            let cx = std::task::Context::from_waker(waker);
            if self.doc.poll(Some(cx)) {
                #[cfg(feature = "accessibility")]
                {
                    if self.doc.has_changes() {
                        self.accessibility.update_tree(&self.doc);
                    }
                }

                self.request_redraw();
                return true;
            }
        }

        false
    }

    pub fn request_redraw(&self) {
        if self.renderer.is_active() {
            self.window.request_redraw();
        }
    }

    pub fn redraw(&mut self) {
        let animation_time = self.current_animation_time();
        self.doc.resolve(animation_time);
        let (width, height) = self.doc.viewport().window_size;
        let scale = self.doc.viewport().scale_f64();
        self.renderer
            .render(|scene| paint_scene(scene, &self.doc, scale, width, height));

        if self.is_visible && self.doc.is_animating() {
            self.request_redraw();
        }
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    #[inline]
    pub fn with_viewport(&mut self, cb: impl FnOnce(&mut Viewport)) {
        let mut viewport = self.doc.viewport_mut();
        cb(&mut viewport);
        drop(viewport);
        let (width, height) = self.doc.viewport().window_size;
        if width > 0 && height > 0 {
            self.renderer.set_size(width, height);
            self.request_redraw();
        }
    }

    #[cfg(feature = "accessibility")]
    pub fn build_accessibility_tree(&mut self) {
        self.accessibility.update_tree(&self.doc);
    }

    pub fn handle_winit_event(&mut self, event: WindowEvent) {
        match event {
            // Window lifecycle events
            WindowEvent::Destroyed => {}
            WindowEvent::ActivationTokenDone { .. } => {},
            WindowEvent::CloseRequested => {
                // Currently handled at the level above in application.rs
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
            }

            // Window size/position events
            WindowEvent::Moved(_) => {}
            WindowEvent::Occluded(is_occluded) => {
                self.is_visible = !is_occluded;
                if self.is_visible {
                    self.request_redraw();
                }
            },
            WindowEvent::Resized(physical_size) => {
                self.with_viewport(|v| v.window_size = (physical_size.width, physical_size.height));
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.with_viewport(|v| v.set_hidpi_scale(scale_factor as f32));
            }

            // Theme events
            WindowEvent::ThemeChanged(theme) => {
                let color_scheme = theme_to_color_scheme(self.theme_override.unwrap_or(theme));
                self.doc.viewport_mut().color_scheme = color_scheme;
            }

            // Text / keyboard events
            WindowEvent::Ime(ime_event) => {
                self.doc.handle_ui_event(UiEvent::Ime(winit_ime_to_blitz(ime_event)));
                self.request_redraw();
            },
            WindowEvent::ModifiersChanged(new_state) => {
                // Store new keyboard modifier (ctrl, shift, etc) state for later use
                self.keyboard_modifiers = new_state;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key_code) = event.physical_key else {
                    return;
                };

                if event.state.is_pressed() {
                    let ctrl = self.keyboard_modifiers.state().control_key();
                    let meta = self.keyboard_modifiers.state().super_key();
                    let alt = self.keyboard_modifiers.state().alt_key();

                    // Ctrl/Super keyboard shortcuts
                    if ctrl | meta {
                        match key_code {
                            KeyCode::Equal => self.doc.viewport_mut().zoom_by(0.1),
                            KeyCode::Minus => self.doc.viewport_mut().zoom_by(-0.1),
                            KeyCode::Digit0 => self.doc.viewport_mut().set_zoom(1.0),
                            _ => {}
                        };
                    }

                    // Alt keyboard shortcuts
                    if alt {
                        match key_code {
                            KeyCode::KeyD => {
                                self.doc.devtools_mut().toggle_show_layout();
                                self.request_redraw();
                            }
                            KeyCode::KeyH => {
                                self.doc.devtools_mut().toggle_highlight_hover();
                                self.request_redraw();
                            }
                            KeyCode::KeyT => self.doc.print_taffy_tree(),
                            _ => {}
                        };
                    }

                }

                // Unmodified keypresses
                let key_event_data = winit_key_event_to_blitz(&event, self.keyboard_modifiers.state());
                let event = if event.state.is_pressed() {
                    UiEvent::KeyDown(key_event_data)
                } else {
                    UiEvent::KeyUp(key_event_data)
                };

                self.doc.handle_ui_event(event);
                self.request_redraw();
            }


            // Mouse/pointer events
            WindowEvent::CursorEntered { /*device_id*/.. } => {}
            WindowEvent::CursorLeft { /*device_id*/.. } => {}
            WindowEvent::CursorMoved { position, .. } => {
                let winit::dpi::LogicalPosition::<f32> { x, y } = position.to_logical(self.window.scale_factor());
                self.mouse_pos = (x, y);
                let event = UiEvent::MouseMove(BlitzMouseButtonEvent {
                    x,
                    y,
                    button: Default::default(),
                    buttons: self.buttons,
                    mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                });
                self.doc.handle_ui_event(event);
            }
            WindowEvent::MouseInput { button, state, .. } => {
                let button = match button {
                    MouseButton::Left => MouseEventButton::Main,
                    MouseButton::Right => MouseEventButton::Secondary,
                    _ => return,
                };

                match state {
                    ElementState::Pressed => self.buttons |= button.into(),
                    ElementState::Released => self.buttons ^= button.into(),
                }

                let event = BlitzMouseButtonEvent {
                    x: self.mouse_pos.0,
                    y: self.mouse_pos.1,
                    button,
                    buttons: self.buttons,
                    mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                };

                let event = match state {
                    ElementState::Pressed => UiEvent::MouseDown(event),
                    ElementState::Released => UiEvent::MouseUp(event),
                };
                self.doc.handle_ui_event(event);
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (scroll_x, scroll_y)= match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x as f64 * 20.0, y as f64 * 20.0),
                    winit::event::MouseScrollDelta::PixelDelta(offsets) => (offsets.x, offsets.y)
                };

                let has_changed = if let Some(hover_node_id) = self.doc.get_hover_node_id() {
                    self.doc.scroll_node_by_has_changed(hover_node_id, scroll_x, scroll_y)
                } else {
                    self.doc.scroll_viewport_by_has_changed(scroll_x, scroll_y)
                };

                if has_changed {
                    self.request_redraw();
                }
            }

            // File events
            WindowEvent::DroppedFile(_) => {}
            WindowEvent::HoveredFile(_) => {}
            WindowEvent::HoveredFileCancelled => {}
            WindowEvent::Focused(_) => {}

            // Touch and motion events
            // Todo implement touch scrolling
            WindowEvent::Touch(_) => {}
            WindowEvent::TouchpadPressure { .. } => {}
            WindowEvent::AxisMotion { .. } => {}
            WindowEvent::PinchGesture { .. } => {},
            WindowEvent::PanGesture { .. } => {},
            WindowEvent::DoubleTapGesture { .. } => {},
            WindowEvent::RotationGesture { .. } => {},
        }
    }
}
