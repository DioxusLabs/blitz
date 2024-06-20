use crate::waker::UserEvent;
use blitz::{RenderState, Renderer, Viewport};
use blitz_dom::DocumentLike;
use winit::keyboard::PhysicalKey;

use std::sync::Arc;
use std::task::Waker;
use vello::Scene;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
use winit::platform::unix::WindowExtUnix;
use winit::{event::WindowEvent, keyboard::KeyCode, keyboard::ModifiersState, window::Window};

#[cfg(not(target_os = "macos"))]
use muda::{AboutMetadata, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

pub(crate) struct View<'s, Doc: DocumentLike> {
    pub(crate) renderer: Renderer<'s, Window, Doc>,
    pub(crate) scene: Scene,
    pub(crate) waker: Option<Waker>,
    /// The state of the keyboard modifiers (ctrl, shift, etc). Winit/Tao don't track these for us so we
    /// need to store them in order to have access to them when processing keypress events
    keyboard_modifiers: ModifiersState,
}

impl<'a, Doc: DocumentLike> View<'a, Doc> {
    pub(crate) fn new(doc: Doc) -> Self {
        Self {
            renderer: Renderer::new(doc),
            scene: Scene::new(),
            waker: None,
            keyboard_modifiers: Default::default(),
        }
    }
}

impl<'a, Doc: DocumentLike> View<'a, Doc> {
    pub(crate) fn poll(&mut self) -> bool {
        match &self.waker {
            None => false,
            Some(waker) => {
                let cx = std::task::Context::from_waker(waker);
                self.renderer.poll(cx)
            }
        }
    }

    pub fn request_redraw(&self) {
        let RenderState::Active(state) = &self.renderer.render_state else {
            return;
        };

        state.window.request_redraw();
    }

    pub fn handle_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::MouseInput {
                // device_id,
                state,
                button,
                // modifiers,
                ..
            } => {
                if state == ElementState::Pressed && button == MouseButton::Left {
                    self.renderer.click();

                    self.request_redraw();
                }
            }

            WindowEvent::Resized(physical_size) => {
                self.renderer
                    .set_size((physical_size.width, physical_size.height));
                self.request_redraw();
            }

            // Store new keyboard modifier (ctrl, shift, etc) state for later use
            WindowEvent::ModifiersChanged(new_state) => {
                self.keyboard_modifiers = new_state.state();
            }

            // todo: if there's an active text input, we want to direct input towards it and translate system emi text
            WindowEvent::KeyboardInput { event, .. } => {
                dbg!(&event);

                match event.physical_key {
                    PhysicalKey::Code(key_code) => {
                        match key_code {
                            KeyCode::Equal => {
                                if self.keyboard_modifiers.control_key()
                                    || self.keyboard_modifiers.super_key()
                                {
                                    self.renderer.zoom(0.1);
                                    self.request_redraw();
                                }
                            }
                            KeyCode::Minus => {
                                if self.keyboard_modifiers.control_key()
                                    || self.keyboard_modifiers.super_key()
                                {
                                    self.renderer.zoom(-0.1);
                                    self.request_redraw();
                                }
                            }
                            KeyCode::Digit0 => {
                                if self.keyboard_modifiers.control_key()
                                    || self.keyboard_modifiers.super_key()
                                {
                                    self.renderer.reset_zoom();
                                    self.request_redraw();
                                }
                            }
                            KeyCode::KeyD => {
                                if event.state == ElementState::Pressed && self.keyboard_modifiers.alt_key()
                                {
                                    self.renderer.devtools.show_layout =
                                        !self.renderer.devtools.show_layout;
                                    self.request_redraw();
                                }
                            }
                            KeyCode::KeyH => {
                                if event.state == ElementState::Pressed && self.keyboard_modifiers.alt_key()
                                {
                                    self.renderer.devtools.highlight_hover =
                                        !self.renderer.devtools.highlight_hover;
                                    self.request_redraw();
                                }
                            }
                            KeyCode::KeyT => {
                                if event.state == ElementState::Pressed && self.keyboard_modifiers.alt_key()
                                {
                                    self.renderer.print_taffy_tree();
                                }
                            }
                            _ => {}
                        }
                    },
                    PhysicalKey::Unidentified(_) => {}
                }
            }
            WindowEvent::Moved(_) => {}
            WindowEvent::CloseRequested => {}
            WindowEvent::Destroyed => {}
            WindowEvent::DroppedFile(_) => {}
            WindowEvent::HoveredFile(_) => {}
            WindowEvent::HoveredFileCancelled => {}
            WindowEvent::Focused(_) => {}
            WindowEvent::CursorMoved {
                // device_id,
                position,
                // modifiers,
                ..
            } => {
                let changed = if let RenderState::Active(state) = &self.renderer.render_state {
                    let winit::dpi::LogicalPosition::<f32> { x, y } = position.to_logical(state.window.scale_factor());

                    self.renderer.mouse_move(x, y)
                } else {
                    false
                };

                if changed {
                    let cursor = self.renderer.get_cursor();

                    if let Some(cursor) = cursor {
                        use style::values::computed::ui::CursorKind;
                        use winit::window::CursorIcon as TaoCursor;
                        let tao_cursor = match cursor {
                            CursorKind::None => todo!("set the cursor to none"),
                            CursorKind::Default => TaoCursor::Default,
                            CursorKind::Pointer => TaoCursor::Pointer,
                            CursorKind::ContextMenu => TaoCursor::ContextMenu,
                            CursorKind::Help => TaoCursor::Help,
                            CursorKind::Progress => TaoCursor::Progress,
                            CursorKind::Wait => TaoCursor::Wait,
                            CursorKind::Cell => TaoCursor::Cell,
                            CursorKind::Crosshair => TaoCursor::Crosshair,
                            CursorKind::Text => TaoCursor::Text,
                            CursorKind::VerticalText => TaoCursor::VerticalText,
                            CursorKind::Alias => TaoCursor::Alias,
                            CursorKind::Copy => TaoCursor::Copy,
                            CursorKind::Move => TaoCursor::Move,
                            CursorKind::NoDrop => TaoCursor::NoDrop,
                            CursorKind::NotAllowed => TaoCursor::NotAllowed,
                            CursorKind::Grab => TaoCursor::Grab,
                            CursorKind::Grabbing => TaoCursor::Grabbing,
                            CursorKind::EResize => TaoCursor::EResize,
                            CursorKind::NResize => TaoCursor::NResize,
                            CursorKind::NeResize => TaoCursor::NeResize,
                            CursorKind::NwResize => TaoCursor::NwResize,
                            CursorKind::SResize => TaoCursor::SResize,
                            CursorKind::SeResize => TaoCursor::SeResize,
                            CursorKind::SwResize => TaoCursor::SwResize,
                            CursorKind::WResize => TaoCursor::WResize,
                            CursorKind::EwResize => TaoCursor::EwResize,
                            CursorKind::NsResize => TaoCursor::NsResize,
                            CursorKind::NeswResize => TaoCursor::NeswResize,
                            CursorKind::NwseResize => TaoCursor::NwseResize,
                            CursorKind::ColResize => TaoCursor::ColResize,
                            CursorKind::RowResize => TaoCursor::RowResize,
                            CursorKind::AllScroll => TaoCursor::AllScroll,
                            CursorKind::ZoomIn => TaoCursor::ZoomIn,
                            CursorKind::ZoomOut => TaoCursor::ZoomOut,
                            CursorKind::Auto => {
                                // todo: we should be the ones determining this based on the UA?
                                // https://developer.mozilla.org/en-US/docs/Web/CSS/cursor


                                TaoCursor::Default
                            },
                        };

                        if let RenderState::Active(state) = &self.renderer.render_state {
                            state.window.set_cursor(tao_cursor);
                            self.request_redraw();
                        }
                    }
                }


            }
            WindowEvent::CursorEntered { /*device_id*/.. } => {}
            WindowEvent::CursorLeft { /*device_id*/.. } => {}
            WindowEvent::MouseWheel {
                // device_id,
                delta,
                // phase,
                // modifiers,
                ..
            } => {
                match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => {
                        self.renderer.scroll_by(y as f64 * 20.0)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(offsets) => {
                        self.renderer.scroll_by(offsets.y)
                    }
                };
                self.request_redraw();
            }

            WindowEvent::TouchpadPressure {
                // device_id,
                // pressure,
                // stage,
                ..
            } => {}
            WindowEvent::AxisMotion {
                // device_id,
                // axis,
                // value,
                ..
            } => {}
            WindowEvent::Touch(_) => {}
            WindowEvent::ScaleFactorChanged {
                // scale_factor,
                // new_inner_size,
                ..
            } => {}
            WindowEvent::ThemeChanged(_) => {}
            _ => {}
        }
    }

    pub fn resume(
        &mut self,
        event_loop: &ActiveEventLoop,
        proxy: &EventLoopProxy<UserEvent>,
        rt: &tokio::runtime::Runtime,
    ) {
        let window_builder = || {
            let window = event_loop
                .create_window(Window::default_attributes().with_inner_size(LogicalSize {
                    width: 800,
                    height: 600,
                }))
                .unwrap();

            #[cfg(target_os = "windows")]
            {
                use winit::raw_window_handle::*;
                if let RawWindowHandle::Win32(handle) = window.window_handle().unwrap().as_raw() {
                    build_menu().init_for_hwnd(handle.hwnd.get()).unwrap();
                }
            }
            #[cfg(target_os = "linux")]
            {
                build_menu()
                    .init_for_gtk_window(window.gtk_window(), window.default_vbox())
                    .unwrap();
            }

            // !TODO - this may not be the right way to do this, but it's a start
            // #[cfg(target_os = "macos")]
            // {
            //     menu_bar.init_for_nsapp();
            //     build_menu().set_as_windows_menu_for_nsapp();
            // }

            let size: winit::dpi::PhysicalSize<u32> = window.inner_size();
            let mut viewport = Viewport::new((size.width, size.height));
            viewport.set_hidpi_scale(window.scale_factor() as _);

            (Arc::from(window), viewport)
        };

        rt.block_on(self.renderer.resume(window_builder));

        let RenderState::Active(state) = &self.renderer.render_state else {
            panic!("Renderer failed to resume");
        };

        self.waker = Some(crate::waker::tao_waker(proxy, state.window.id()));
        self.renderer.render(&mut self.scene);
    }

    pub fn suspend(&mut self) {
        self.waker = None;
        self.renderer.suspend();
    }
}

#[cfg(not(target_os = "macos"))]
fn build_menu() -> Menu {
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

    menu
}
