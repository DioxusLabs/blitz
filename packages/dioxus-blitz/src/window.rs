use crate::accessibility::AccessibilityState;
use crate::stylo_to_winit;
use crate::waker::{BlitzEvent, BlitzWindowEvent};
use blitz::{Devtools, Renderer};
use blitz_dom::events::{EventData, RendererEvent};
use blitz_dom::{DocumentLike, Viewport};
use winit::keyboard::PhysicalKey;

#[allow(unused)]
use wgpu::rwh::HasWindowHandle;

use std::sync::Arc;
use std::task::Waker;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{WindowAttributes, WindowId};
use winit::{event::WindowEvent, keyboard::KeyCode, keyboard::ModifiersState, window::Window};

pub struct WindowConfig<Doc: DocumentLike> {
    doc: Doc,
    attributes: WindowAttributes,
}

impl<Doc: DocumentLike> WindowConfig<Doc> {
    pub fn new(doc: Doc, width: f32, height: f32) -> Self {
        WindowConfig {
            doc,
            attributes: Window::default_attributes().with_inner_size(LogicalSize { width, height }),
        }
    }

    pub fn with_attributes(doc: Doc, attributes: WindowAttributes) -> Self {
        WindowConfig { doc, attributes }
    }
}

pub(crate) struct View<'s, Doc: DocumentLike> {
    pub(crate) renderer: Renderer<'s, Window>,
    pub(crate) dom: Doc,
    pub(crate) waker: Option<Waker>,

    event_loop_proxy: EventLoopProxy<BlitzEvent>,
    window: Arc<Window>,

    /// The actual viewport of the page that we're getting a glimpse of.
    /// We need this since the part of the page that's being viewed might not be the page in its entirety.
    /// This will let us opt of rendering some stuff
    viewport: Viewport,

    /// The state of the keyboard modifiers (ctrl, shift, etc). Winit/Tao don't track these for us so we
    /// need to store them in order to have access to them when processing keypress events
    keyboard_modifiers: ModifiersState,
    pub devtools: Devtools,
    mouse_pos: (f32, f32),

    #[cfg(feature = "accessibility")]
    /// Accessibility adapter for `accesskit`.
    accessibility: AccessibilityState,

    /// Main menu bar of this view's window.
    #[cfg(feature = "menu")]
    _menu: muda::Menu,
}

impl<'a, Doc: DocumentLike> View<'a, Doc> {
    pub(crate) fn init(
        config: WindowConfig<Doc>,
        event_loop: &ActiveEventLoop,
        proxy: &EventLoopProxy<BlitzEvent>,
    ) -> Self {
        let winit_window = Arc::from(event_loop.create_window(config.attributes).unwrap());

        let size = winit_window.inner_size();
        let mut viewport = Viewport::new((size.width, size.height));
        viewport.set_hidpi_scale(winit_window.scale_factor() as f32);

        Self {
            renderer: Renderer::new(winit_window.clone()),
            waker: None,
            keyboard_modifiers: Default::default(),

            event_loop_proxy: proxy.clone(),
            window: winit_window.clone(),
            dom: config.doc,
            viewport,
            devtools: Default::default(),
            mouse_pos: Default::default(),

            #[cfg(feature = "accessibility")]
            accessibility: AccessibilityState::new(&winit_window, proxy.clone()),

            #[cfg(feature = "menu")]
            _menu: init_menu(
                #[cfg(target_os = "windows")]
                &winit_window,
            ),
        }
    }
}

impl<'a, Doc: DocumentLike> View<'a, Doc> {
    pub(crate) fn poll(&mut self) -> bool {
        match &self.waker {
            None => false,
            Some(waker) => {
                let cx = std::task::Context::from_waker(waker);
                if self.dom.poll(cx) {
                    #[cfg(feature = "accessibility")]
                    {
                        // TODO send fine grained accessibility tree updates.
                        let changed = std::mem::take(&mut self.dom.as_mut().changed);
                        if !changed.is_empty() {
                            self.accessibility.build_tree(self.dom.as_ref());
                        }
                    }

                    true
                } else {
                    false
                }
            }
        }
    }

    pub fn request_redraw(&self) {
        if self.renderer.is_active() {
            self.window.request_redraw();
        }
    }

    pub fn redraw(&mut self) {
        self.dom.as_mut().resolve();
        self.renderer
            .render(self.dom.as_ref(), self.viewport.scale_f64(), self.devtools);
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
            self.dom.as_mut().set_scale(self.viewport.scale());
            self.dom
                .as_mut()
                .set_stylist_device(self.viewport.make_device());
        }
    }

    pub fn kick_renderer_viewport(&mut self) {
        let (width, height) = self.viewport.window_size;
        if width > 0 && height > 0 {
            self.renderer.set_size(width, height);
        }
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let x = x / self.viewport.zoom();
        let y = (y - self.dom.as_ref().scroll_offset as f32) / self.viewport.zoom();

        // println!("Mouse move: ({}, {})", x, y);
        // println!("Unscaled: ({}, {})",);

        self.dom.as_mut().set_hover_to(x, y)
    }

    pub fn click(&mut self, button: &str) {
        let Some(node_id) = self.dom.as_ref().get_hover_node_id() else {
            return;
        };

        if !self.renderer.is_active() {
            return;
        };

        if self.devtools.highlight_hover {
            let mut node = self.dom.as_ref().get_node(node_id).unwrap();
            if button == "right" {
                if let Some(parent_id) = node.parent {
                    node = self.dom.as_ref().get_node(parent_id).unwrap();
                }
            }
            self.dom.as_ref().debug_log_node(node.id);
            self.devtools.highlight_hover = false;
        } else {
            // Not debug mode. Handle click as usual
            if button == "left" {
                // If we hit a node, then we collect the node to its parents, check for listeners, and then
                // call those listeners
                self.dom.handle_event(RendererEvent {
                    name: "click".to_string(),
                    target: node_id,
                    data: EventData::Click {
                        x: self.mouse_pos.0 as f64,
                        y: self.mouse_pos.1 as f64,
                    },
                });
            }
        }
    }

    pub fn handle_winit_event(&mut self, event: WindowEvent) {
        match event {
            // Window lifecycle events
            WindowEvent::Destroyed => {}
            WindowEvent::ActivationTokenDone { .. } => {},
            WindowEvent::CloseRequested => {
                // Currently handled at the level above in lib.rs
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
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged {
                // scale_factor,
                // new_inner_size,
                ..
            } => {}

            // Theme events
            WindowEvent::ThemeChanged(_) => {
                // TODO: dark mode / light mode support
            }

            // Text / keyboard events
            WindowEvent::Ime(_) => {
                // TODO: handle IME events
            },
            WindowEvent::ModifiersChanged(new_state) => {
                // Store new keyboard modifier (ctrl, shift, etc) state for later use
                self.keyboard_modifiers = new_state.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key_code) = event.physical_key else {
                    return;
                };
                if !event.state.is_pressed() {
                    return;
                }

                let ctrl = self.keyboard_modifiers.control_key();
                let meta = self.keyboard_modifiers.super_key();
                let alt = self.keyboard_modifiers.alt_key();

                // Ctrl/Super keyboard shortcuts
                if ctrl | meta {
                    match key_code {
                        KeyCode::Equal => {
                            *self.viewport.zoom_mut() += 0.1;
                            self.kick_dom_viewport();
                            self.request_redraw();
                        }
                        KeyCode::Minus => {
                            *self.viewport.zoom_mut() -= 0.1;
                            self.kick_dom_viewport();
                            self.request_redraw();
                        }
                        KeyCode::Digit0 => {
                            *self.viewport.zoom_mut() = 1.0;
                            self.kick_dom_viewport();
                            self.request_redraw();
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
                            self.dom.as_ref().print_taffy_tree();
                        }
                        _ => {}
                    };
                }

                // Unmodified keypresses
                match key_code {
                    KeyCode::Tab if event.state.is_pressed() => {
                        self.dom.as_mut().focus_next_node();
                        self.request_redraw();
                    }
                    _ => {
                        // TODO: handle regular keypresses for text input
                    }
                }
            }


            // Mouse/pointer events
            WindowEvent::CursorEntered { /*device_id*/.. } => {}
            WindowEvent::CursorLeft { /*device_id*/.. } => {}
            WindowEvent::CursorMoved { position, .. } => {
                let changed = if self.renderer.is_active() {
                    let winit::dpi::LogicalPosition::<f32> { x, y } = position.to_logical(self.window.scale_factor());
                    self.mouse_move(x, y)
                } else {
                    false
                };

                if changed {
                    let cursor = self.dom.as_ref().get_cursor();
                    if let Some(cursor) = cursor {
                        if self.renderer.is_active() {
                            self.window.set_cursor(stylo_to_winit::cursor(cursor));
                            self.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::MouseInput { button, state, .. } => {
                if state == ElementState::Pressed && matches!(button, MouseButton::Left | MouseButton::Right) {
                    self.click(match button {
                        MouseButton::Left => "left",
                        MouseButton::Right => "right",
                        _ => unreachable!(),

                    });

                    self.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => {
                        self.dom.as_mut().scroll_by(y as f64 * 20.0)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(offsets) => {
                        self.dom.as_mut().scroll_by(offsets.y)
                    }
                };
                self.request_redraw();
            }

            // File events
            WindowEvent::DroppedFile(_) => {}
            WindowEvent::HoveredFile(_) => {}
            WindowEvent::HoveredFileCancelled => {}
            WindowEvent::Focused(_) => {}

            // Touch and motion events
            WindowEvent::Touch(_) => {}
            WindowEvent::TouchpadPressure { .. } => {}
            WindowEvent::AxisMotion { .. } => {}
            WindowEvent::PinchGesture { .. } => {},
            WindowEvent::PanGesture { .. } => {},
            WindowEvent::DoubleTapGesture { .. } => {},
            WindowEvent::RotationGesture { .. } => {},
        }
    }

    pub fn handle_blitz_event(&mut self, event: BlitzWindowEvent) {
        match event {
            BlitzWindowEvent::Poll => {
                if self.poll() {
                    self.request_redraw();
                }
            }
            #[cfg(feature = "accessibility")]
            BlitzWindowEvent::Accessibility(accessibility_event) => {
                match &*accessibility_event {
                    accesskit_winit::WindowEvent::InitialTreeRequested => {
                        self.accessibility.build_tree(self.dom.as_ref());
                    }
                    accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                        // TODO
                    }
                    accesskit_winit::WindowEvent::ActionRequested(_req) => {
                        // TODO
                    }
                }
            }
        }
    }

    pub fn resume(&mut self, rt: &tokio::runtime::Runtime) {
        let device = self.viewport.make_device();
        self.dom.as_mut().set_stylist_device(device);
        self.dom.as_mut().set_scale(self.viewport.scale());

        rt.block_on(self.renderer.resume(&self.viewport));

        if !self.renderer.is_active() {
            panic!("Renderer failed to resume");
        };

        self.dom.as_mut().resolve();

        self.waker = Some(crate::waker::tao_waker(
            &self.event_loop_proxy,
            self.window_id(),
        ));
        self.renderer
            .render(self.dom.as_ref(), self.viewport.scale_f64(), self.devtools);
    }

    pub fn suspend(&mut self) {
        self.waker = None;
        self.renderer.suspend();
    }
}

/// Initialize the default menu bar.
#[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
pub fn init_menu(#[cfg(target_os = "windows")] window: &Window) -> muda::Menu {
    use muda::{AboutMetadata, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

    let menu = Menu::new();

    // Build the about section
    let about = Submenu::new("About", true);
    about
        .append_items(&[
            &PredefinedMenuItem::about("Dioxus".into(), Option::from(AboutMetadata::default())),
            &MenuItem::with_id(MenuId::new("dev.show_layout"), "Show layout", true, None),
        ])
        .unwrap();
    menu.append(&about).unwrap();

    #[cfg(target_os = "windows")]
    {
        use winit::raw_window_handle::*;
        if let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw() {
            menu.init_for_hwnd(handle.hwnd.get()).unwrap();
        }
    }

    // todo: menu on linux
    // #[cfg(target_os = "linux")]
    // {
    //     use winit::platform::unix::WindowExtUnix;
    //     menu.init_for_gtk_window(window.gtk_window(), window.default_vbox())
    //         .unwrap();
    // }

    #[cfg(target_os = "macos")]
    {
        menu.init_for_nsapp();
    }

    menu
}
