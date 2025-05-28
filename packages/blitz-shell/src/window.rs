use crate::convert_events::{
    winit_ime_to_blitz, winit_key_event_to_blitz, winit_modifiers_to_kbt_modifiers,
};
use crate::event::{BlitzShellEvent, create_waker};
use anyrender::WindowRenderer;
use blitz_dom::Document;
use blitz_paint::paint_scene;
use blitz_traits::events::UiEvent;
use blitz_traits::{
    BlitzMouseButtonEvent, ColorScheme, MouseEventButton, MouseEventButtons, Viewport,
};
use winit::keyboard::PhysicalKey;

use std::marker::PhantomData;
use std::sync::Arc;
use std::task::Waker;
use winit::event::{ElementState, MouseButton};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{Theme, WindowAttributes, WindowId};
use winit::{event::Modifiers, event::WindowEvent, keyboard::KeyCode, window::Window};

#[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
use crate::menu::init_menu;

#[cfg(feature = "accessibility")]
use crate::accessibility::AccessibilityState;

pub struct WindowConfig<Rend: WindowRenderer> {
    doc: Box<dyn Document>,
    attributes: WindowAttributes,
    rend: PhantomData<Rend>,
}

impl<Rend: WindowRenderer> WindowConfig<Rend> {
    pub fn new(doc: Box<dyn Document>) -> Self {
        Self::with_attributes(doc, Window::default_attributes())
    }

    pub fn with_attributes(doc: Box<dyn Document>, attributes: WindowAttributes) -> Self {
        WindowConfig {
            doc,
            attributes,
            rend: PhantomData,
        }
    }
}

pub struct View<Rend: WindowRenderer> {
    pub doc: Box<dyn Document>,

    pub(crate) renderer: Rend,
    pub(crate) waker: Option<Waker>,

    event_loop_proxy: EventLoopProxy<BlitzShellEvent>,
    window: Arc<Window>,

    /// The actual viewport of the page that we're getting a glimpse of.
    /// We need this since the part of the page that's being viewed might not be the page in its entirety.
    /// This will let us opt of rendering some stuff
    viewport: Viewport,

    /// The state of the keyboard modifiers (ctrl, shift, etc). Winit/Tao don't track these for us so we
    /// need to store them in order to have access to them when processing keypress events
    theme_override: Option<Theme>,
    keyboard_modifiers: Modifiers,
    buttons: MouseEventButtons,
    mouse_pos: (f32, f32),

    #[cfg(feature = "accessibility")]
    /// Accessibility adapter for `accesskit`.
    accessibility: AccessibilityState,

    /// Main menu bar of this view's window.
    /// Field is _ prefixed because it is never read. But it needs to be stored here to prevent it from dropping.
    #[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
    _menu: muda::Menu,
}

impl<Rend: WindowRenderer> View<Rend> {
    pub fn init(
        config: WindowConfig<Rend>,
        event_loop: &ActiveEventLoop,
        proxy: &EventLoopProxy<BlitzShellEvent>,
    ) -> Self {
        let winit_window = Arc::from(event_loop.create_window(config.attributes).unwrap());

        // TODO: make this conditional on text input focus
        winit_window.set_ime_allowed(true);

        // Create viewport
        let size = winit_window.inner_size();
        let scale = winit_window.scale_factor() as f32;
        let theme = winit_window.theme().unwrap_or(Theme::Light);
        let color_scheme = theme_to_color_scheme(theme);
        let viewport = Viewport::new(size.width, size.height, scale, color_scheme);

        Self {
            renderer: Rend::new(winit_window.clone()),
            waker: None,
            keyboard_modifiers: Default::default(),

            event_loop_proxy: proxy.clone(),
            window: winit_window.clone(),
            doc: config.doc,
            viewport,
            theme_override: None,
            buttons: MouseEventButtons::None,
            mouse_pos: Default::default(),
            #[cfg(feature = "accessibility")]
            accessibility: AccessibilityState::new(&winit_window, proxy.clone()),

            #[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
            _menu: init_menu(&winit_window),
        }
    }

    pub fn replace_document(&mut self, new_doc: Box<dyn Document>, retain_scroll_position: bool) {
        let scroll = self.doc.viewport_scroll();

        self.doc = new_doc;
        self.kick_viewport();
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
        color_scheme_to_theme(self.viewport.color_scheme)
    }

    pub fn set_theme_override(&mut self, theme: Option<Theme>) {
        self.theme_override = theme;
        let theme = theme.or(self.window.theme()).unwrap_or(Theme::Light);
        self.viewport.color_scheme = theme_to_color_scheme(theme);
        self.kick_viewport();
        self.request_redraw();
    }

    pub fn downcast_doc_mut<T: 'static>(&mut self) -> &mut T {
        self.doc.as_any_mut().downcast_mut::<T>().unwrap()
    }
}

impl<Rend: WindowRenderer> View<Rend> {
    pub fn resume(&mut self) {
        // Resolve dom
        self.doc.set_viewport(self.viewport.clone());
        self.doc.resolve();

        // Resume renderer
        let (width, height) = self.viewport.window_size;
        self.renderer.resume(width, height);
        if !self.renderer.is_active() {
            panic!("Renderer failed to resume");
        };

        // Render
        self.renderer.render(|scene| {
            paint_scene(scene, &self.doc, self.viewport.scale_f64(), width, height)
        });

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
            if self.doc.poll(cx) {
                #[cfg(feature = "accessibility")]
                {
                    // TODO send fine grained accessibility tree updates.
                    let changed = std::mem::take(&mut self.doc.changed);
                    if !changed.is_empty() {
                        self.accessibility.build_tree(&self.doc);
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
        self.doc.resolve();
        let (width, height) = self.viewport.window_size;
        self.renderer.render(|scene| {
            paint_scene(scene, &self.doc, self.viewport.scale_f64(), width, height)
        });
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub fn kick_viewport(&mut self) {
        self.kick_dom_viewport();
        self.doc.scroll_viewport_by(0.0, 0.0); // Clamp scroll offset
        self.kick_renderer_viewport();
    }

    pub fn kick_dom_viewport(&mut self) {
        let (width, height) = self.viewport.window_size;
        if width > 0 && height > 0 {
            self.doc.set_viewport(self.viewport.clone());
            self.request_redraw();
        }
    }

    pub fn kick_renderer_viewport(&mut self) {
        let (width, height) = self.viewport.window_size;
        if width > 0 && height > 0 {
            self.renderer.set_size(width, height);
            self.request_redraw();
        }
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) {
        self.mouse_pos = (x, y);
        let event = UiEvent::MouseMove(BlitzMouseButtonEvent {
            x,
            y,
            button: Default::default(),
            buttons: self.buttons,
            mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
        });
        self.doc.handle_event(event);
    }

    #[cfg(feature = "accessibility")]
    pub fn build_accessibility_tree(&mut self) {
        self.accessibility.build_tree(&self.doc);
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
            WindowEvent::Occluded(_) => {},
            WindowEvent::Resized(physical_size) => {
                self.viewport.window_size = (physical_size.width, physical_size.height);
                self.kick_viewport();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.viewport.set_hidpi_scale(scale_factor as f32);
                self.kick_viewport();
            }

            // Theme events
            WindowEvent::ThemeChanged(theme) => {
                self.viewport.color_scheme = theme_to_color_scheme(self.theme_override.unwrap_or(theme));
                self.kick_viewport();
            }

            // Text / keyboard events
            WindowEvent::Ime(ime_event) => {
                self.doc.handle_event(UiEvent::Ime(winit_ime_to_blitz(ime_event)));
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
                            KeyCode::Equal => {
                                *self.viewport.zoom_mut() += 0.1;
                                self.kick_dom_viewport();
                            }
                            KeyCode::Minus => {
                                *self.viewport.zoom_mut() -= 0.1;
                                self.kick_dom_viewport();
                            }
                            KeyCode::Digit0 => {
                                *self.viewport.zoom_mut() = 1.0;
                                self.kick_dom_viewport();
                            }
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
                            KeyCode::KeyT => {
                                self.doc.print_taffy_tree();
                            }
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

                self.doc.handle_event(event);
                self.request_redraw();
            }


            // Mouse/pointer events
            WindowEvent::CursorEntered { /*device_id*/.. } => {}
            WindowEvent::CursorLeft { /*device_id*/.. } => {}
            WindowEvent::CursorMoved { position, .. } => {
                let winit::dpi::LogicalPosition::<f32> { x, y } = position.to_logical(self.window.scale_factor());
                self.mouse_move(x, y);
                self.request_redraw();

                // TODO cursor_changed event
                //
                // if changed {
                //     let cursor = self.doc.get_cursor();
                //     if let Some(cursor) = cursor {
                //             self.window.set_cursor(cursor);
                //             self.request_redraw();
                //     }
                // }
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
                self.doc.handle_event(event);
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (scroll_x, scroll_y)= match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x as f64 * 20.0, y as f64 * 20.0),
                    winit::event::MouseScrollDelta::PixelDelta(offsets) => (offsets.x, offsets.y)
                };

                if let Some(hover_node_id)= self.doc.get_hover_node_id() {
                    self.doc.scroll_node_by(hover_node_id, scroll_x, scroll_y);
                } else {
                    self.doc.scroll_viewport_by(scroll_x, scroll_y);
                }
                self.request_redraw();
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

fn theme_to_color_scheme(theme: Theme) -> ColorScheme {
    match theme {
        Theme::Light => ColorScheme::Light,
        Theme::Dark => ColorScheme::Dark,
    }
}

fn color_scheme_to_theme(scheme: ColorScheme) -> Theme {
    match scheme {
        ColorScheme::Light => Theme::Light,
        ColorScheme::Dark => Theme::Dark,
    }
}
