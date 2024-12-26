use crate::event::{create_waker, BlitzEvent};
use blitz_dom::events::{EventData, RendererEvent};
use blitz_dom::{DocumentLike, DocumentRenderer};
use blitz_traits::{ColorScheme, Devtools, Viewport};
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

pub struct WindowConfig<Doc: DocumentLike, Rend: DocumentRenderer> {
    doc: Doc,
    attributes: WindowAttributes,
    rend: PhantomData<Rend>,
}

impl<Doc: DocumentLike, Rend: DocumentRenderer> WindowConfig<Doc, Rend> {
    pub fn new(doc: Doc) -> Self {
        Self::with_attributes(doc, Window::default_attributes())
    }

    pub fn with_attributes(doc: Doc, attributes: WindowAttributes) -> Self {
        WindowConfig {
            doc,
            attributes,
            rend: PhantomData,
        }
    }
}

pub struct View<Doc: DocumentLike, Rend: DocumentRenderer> {
    pub doc: Doc,

    pub(crate) renderer: Rend,
    pub(crate) waker: Option<Waker>,

    event_loop_proxy: EventLoopProxy<BlitzEvent>,
    window: Arc<Window>,

    /// The actual viewport of the page that we're getting a glimpse of.
    /// We need this since the part of the page that's being viewed might not be the page in its entirety.
    /// This will let us opt of rendering some stuff
    viewport: Viewport,

    /// The state of the keyboard modifiers (ctrl, shift, etc). Winit/Tao don't track these for us so we
    /// need to store them in order to have access to them when processing keypress events
    pub devtools: Devtools,
    theme_override: Option<Theme>,
    keyboard_modifiers: Modifiers,
    mouse_pos: (f32, f32),
    dom_mouse_pos: (f32, f32),
    mouse_down_node: Option<usize>,

    #[cfg(feature = "accessibility")]
    /// Accessibility adapter for `accesskit`.
    accessibility: AccessibilityState,

    /// Main menu bar of this view's window.
    /// Field is _ prefixed because it is never read. But it needs to be stored here to prevent it from dropping.
    #[cfg(feature = "menu")]
    _menu: muda::Menu,
}

impl<Doc: DocumentLike, Rend: DocumentRenderer> View<Doc, Rend> {
    pub(crate) fn init(
        config: WindowConfig<Doc, Rend>,
        event_loop: &ActiveEventLoop,
        proxy: &EventLoopProxy<BlitzEvent>,
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
            devtools: Default::default(),
            theme_override: None,
            mouse_pos: Default::default(),
            dom_mouse_pos: Default::default(),
            mouse_down_node: None,

            #[cfg(feature = "accessibility")]
            accessibility: AccessibilityState::new(&winit_window, proxy.clone()),

            #[cfg(feature = "menu")]
            _menu: init_menu(&winit_window),
        }
    }

    pub fn replace_document(&mut self, mut new_doc: Doc) {
        let scroll = self.doc.as_ref().viewport_scroll();
        new_doc.as_mut().set_viewport_scroll(scroll);
        self.doc = new_doc;
        self.kick_viewport();
        self.poll();
        self.request_redraw();
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
}

impl<Doc: DocumentLike, Rend: DocumentRenderer> View<Doc, Rend> {
    pub fn resume(&mut self) {
        // Resolve dom
        self.doc.as_mut().set_viewport(self.viewport.clone());
        self.doc.as_mut().resolve();

        // Resume renderer
        self.renderer.resume(&self.viewport);
        if !self.renderer.is_active() {
            panic!("Renderer failed to resume");
        };

        // Render
        let (width, height) = self.viewport.window_size;
        self.renderer.render(
            self.doc.as_ref(),
            self.viewport.scale_f64(),
            width,
            height,
            self.devtools,
        );

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
                    let changed = std::mem::take(&mut self.doc.as_mut().changed);
                    if !changed.is_empty() {
                        self.accessibility.build_tree(self.doc.as_ref());
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
        self.doc.as_mut().resolve();
        let (width, height) = self.viewport.window_size;
        self.renderer.render(
            self.doc.as_ref(),
            self.viewport.scale_f64(),
            width,
            height,
            self.devtools,
        );
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub fn kick_viewport(&mut self) {
        self.kick_dom_viewport();
        self.kick_renderer_viewport();
    }

    pub fn kick_dom_viewport(&mut self) {
        let (width, height) = self.viewport.window_size;
        if width > 0 && height > 0 {
            self.doc.as_mut().set_viewport(self.viewport.clone());
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

    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let viewport_scroll = self.doc.as_ref().viewport_scroll();
        let dom_x = x + viewport_scroll.x as f32 / self.viewport.zoom();
        let dom_y = y + viewport_scroll.y as f32 / self.viewport.zoom();

        // println!("Mouse move: ({}, {})", x, y);
        // println!("Unscaled: ({}, {})",);

        self.mouse_pos = (x, y);
        self.dom_mouse_pos = (dom_x, dom_y);
        self.doc.as_mut().set_hover_to(dom_x, dom_y)
    }

    pub fn mouse_down(&mut self, button: &str) {
        let Some(node_id) = self.doc.as_ref().get_hover_node_id() else {
            return;
        };

        self.doc.as_mut().active_node();

        // If we hit a node, then we collect the node to its parents, check for listeners, and then
        // call those listeners
        self.doc.handle_event(RendererEvent {
            target: node_id,
            data: EventData::MouseDown {
                x: self.dom_mouse_pos.0,
                y: self.dom_mouse_pos.1,
                mods: self.keyboard_modifiers,
            },
        });

        self.mouse_down_node = Some(node_id);
    }

    pub fn mouse_up(&mut self, button: &str) {
        let Some(node_id) = self.doc.as_ref().get_hover_node_id() else {
            return;
        };

        self.doc.as_mut().unactive_node();

        // If we hit a node, then we collect the node to its parents, check for listeners, and then
        // call those listeners
        self.doc.handle_event(RendererEvent {
            target: node_id,
            data: EventData::MouseUp {
                x: self.dom_mouse_pos.0,
                y: self.dom_mouse_pos.1,
                mods: self.keyboard_modifiers,
            },
        });

        if self.mouse_down_node == Some(node_id) {
            self.click(button);
        }
    }

    pub fn click(&mut self, button: &str) {
        let Some(node_id) = self.doc.as_ref().get_hover_node_id() else {
            return;
        };

        if self.devtools.highlight_hover {
            let mut node = self.doc.as_ref().get_node(node_id).unwrap();
            if button == "right" {
                if let Some(parent_id) = node.layout_parent.get() {
                    node = self.doc.as_ref().get_node(parent_id).unwrap();
                }
            }
            self.doc.as_ref().debug_log_node(node.id);
            self.devtools.highlight_hover = false;
        } else {
            // Not debug mode. Handle click as usual
            if button == "left" {
                // If we hit a node, then we collect the node to its parents, check for listeners, and then
                // call those listeners
                self.doc.handle_event(RendererEvent {
                    target: node_id,
                    data: EventData::Click {
                        x: self.dom_mouse_pos.0,
                        y: self.dom_mouse_pos.1,
                        mods: self.keyboard_modifiers,
                    },
                });
            }
        }
    }

    #[cfg(feature = "accessibility")]
    pub fn build_accessibility_tree(&mut self) {
        self.accessibility.build_tree(self.doc.as_ref());
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
                if let Some(target) = self.doc.as_ref().get_focussed_node_id() {
                    self.doc.handle_event(RendererEvent { target, data: EventData::Ime(ime_event) });
                    self.request_redraw();
                }
            },
            WindowEvent::ModifiersChanged(new_state) => {
                // Store new keyboard modifier (ctrl, shift, etc) state for later use
                self.keyboard_modifiers = new_state;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key_code) = event.physical_key else {
                    return;
                };
                if !event.state.is_pressed() {
                    return;
                }

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
                            self.devtools.show_layout = !self.devtools.show_layout;
                            self.request_redraw();
                        }
                        KeyCode::KeyH => {
                            self.devtools.highlight_hover = !self.devtools.highlight_hover;
                            self.request_redraw();
                        }
                        KeyCode::KeyT => {
                            self.doc.as_ref().print_taffy_tree();
                        }
                        _ => {}
                    };
                }

                // Unmodified keypresses
                match key_code {
                    KeyCode::Tab if event.state.is_pressed() => {
                        self.doc.as_mut().focus_next_node();
                        self.request_redraw();
                    }
                    _ => {
                        if let Some(focus_node_id) = self.doc.as_ref().get_focussed_node_id() {
                            self.doc.handle_event(RendererEvent {
                                target: focus_node_id,
                                data: EventData::KeyPress { event, mods: self.keyboard_modifiers }
                            });
                            self.request_redraw();
                        }
                    }
                }
            }


            // Mouse/pointer events
            WindowEvent::CursorEntered { /*device_id*/.. } => {}
            WindowEvent::CursorLeft { /*device_id*/.. } => {}
            WindowEvent::CursorMoved { position, .. } => {
                let winit::dpi::LogicalPosition::<f32> { x, y } = position.to_logical(self.window.scale_factor());
                let changed = self.mouse_move(x, y);

                if changed {
                    let cursor = self.doc.as_ref().get_cursor();
                    if let Some(cursor) = cursor {
                            self.window.set_cursor(cursor);
                            self.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput { button, state, .. } => {
                if matches!(button, MouseButton::Left | MouseButton::Right) {
                    let button = match button {
                        MouseButton::Left => "left",
                        MouseButton::Right => "right",
                        _ => unreachable!(),
                    };

                    match state {
                        ElementState::Pressed => self.mouse_down(button),
                        ElementState::Released => self.mouse_up(button)
                    }

                    self.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (scroll_x, scroll_y)= match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x as f64 * 20.0, y as f64 * 20.0),
                    winit::event::MouseScrollDelta::PixelDelta(offsets) => (offsets.x, offsets.y)
                };

                if let Some(hover_node_id)= self.doc.as_ref().get_hover_node_id() {
                    self.doc.as_mut().scroll_node_by(hover_node_id, scroll_x, scroll_y);
                } else {
                    self.doc.as_mut().scroll_viewport_by(scroll_x, scroll_y);
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
