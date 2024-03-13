use super::Config;
use crate::waker::UserWindowEvent;
use blitz::{RenderState, Renderer, Viewport};
use blitz_dom::Document;
use dioxus::dioxus_core::{ComponentFunction, VirtualDom};
use dioxus_ssr::config;
use futures_util::{pin_mut, FutureExt};
use muda::{AboutMetadata, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::sync::Arc;
use std::task::Waker;
use tao::dpi::LogicalSize;
use tao::event::{ElementState, MouseButton};
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
#[cfg(target_os = "windows")]
use tao::platform::windows::WindowExtWindows;
use tao::{
    event::WindowEvent,
    keyboard::KeyCode,
    keyboard::ModifiersState,
    window::{Window, WindowBuilder},
};
use vello::Scene;

pub(crate) struct View<'s> {
    pub(crate) renderer: Renderer<'s, Window>,
    pub(crate) vdom: VirtualDom,
    pub(crate) scene: Scene,
    pub(crate) waker: Option<Waker>,
    /// The state of the keyboard modifiers (ctrl, shift, etc). Winit/Tao don't track these for us so we
    /// need to store them in order to have access to them when processing keypress events
    keyboard_modifiers: ModifiersState,
}

impl<'a> View<'a> {
    pub(crate) fn new<P: 'static + Clone, M: 'static>(
        root: impl ComponentFunction<P, M>,
        props: P,
        cfg: &Config,
    ) -> Self {
        // Spin up the virtualdom
        // We're going to need to hit it with a special waker
        let mut vdom = VirtualDom::new_with_props(root, props);
        vdom.rebuild_in_place();
        let markup = dioxus_ssr::render(&vdom);

        // TODO: Don't render dioxus via static html
        Self::from_html(&markup, cfg)
    }

    pub(crate) fn from_html(html: &str, cfg: &Config) -> Self {
        // Spin up the virtualdom and include the default stylesheet
        let mut dom = Document::new(Viewport::new((0, 0)).make_device());

        // Set base url if configured
        if let Some(url) = &cfg.base_url {
            dom.set_base_url(&url);
        }

        // Include default and user-specified stylesheets
        dom.add_stylesheet(include_str!("./default.css"));
        for ss in &cfg.stylesheets {
            dom.add_stylesheet(&ss);
        }

        // Populate dom with HTML
        dom.write(&html);

        Self {
            renderer: Renderer::new(dom),
            vdom: VirtualDom::new(|| None),
            scene: Scene::new(),
            waker: None,
            keyboard_modifiers: Default::default(),
        }
    }

    pub(crate) fn poll(&mut self) {
        match &self.waker {
            None => {}
            Some(waker) => {
                let mut cx = std::task::Context::from_waker(waker);

                loop {
                    {
                        let fut = self.vdom.wait_for_work();
                        pin_mut!(fut);

                        match fut.poll_unpin(&mut cx) {
                            std::task::Poll::Ready(_) => {}
                            std::task::Poll::Pending => break,
                        }
                    }

                    // let edits = self.vdom.render_immediate();

                    // apply the mutations to the actual dom

                    // send_edits(view.dom.render_immediate(), &view.desktop_context.webview);
                }
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
                device_id,
                state,
                button,
                modifiers,
            } => {
                if state == ElementState::Pressed && button == MouseButton::Left {
                    self.renderer.click()
                }
            }

            WindowEvent::Resized(physical_size) => {
                self.renderer
                    .set_size((physical_size.width, physical_size.height));
                self.request_redraw();
            }

            // Store new keyboard modifier (ctrl, shift, etc) state for later use
            WindowEvent::ModifiersChanged(new_state) => {
                self.keyboard_modifiers = new_state;
            }

            // todo: if there's an active text input, we want to direct input towards it and translate system emi text
            WindowEvent::KeyboardInput { event, .. } => {
                dbg!(&event);

                match event.physical_key {
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
            }
            WindowEvent::Moved(_) => {}
            WindowEvent::CloseRequested => {}
            WindowEvent::Destroyed => {}
            WindowEvent::DroppedFile(_) => {}
            WindowEvent::HoveredFile(_) => {}
            WindowEvent::HoveredFileCancelled => {}
            WindowEvent::ReceivedImeText(_) => {}
            WindowEvent::Focused(_) => {}
            WindowEvent::CursorMoved {
                device_id,
                position,
                modifiers,
            } => {
                let tao::dpi::LogicalPosition::<f32> { x, y } = position.to_logical(2.0);
                if self.renderer.mouse_move(x, y) {
                    self.request_redraw();
                }
            }
            WindowEvent::CursorEntered { device_id } => {}
            WindowEvent::CursorLeft { device_id } => {}
            WindowEvent::MouseWheel {
                device_id,
                delta,
                phase,
                modifiers,
            } => {}

            WindowEvent::TouchpadPressure {
                device_id,
                pressure,
                stage,
            } => {}
            WindowEvent::AxisMotion {
                device_id,
                axis,
                value,
            } => {}
            WindowEvent::Touch(_) => {}
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                new_inner_size,
            } => {}
            WindowEvent::ThemeChanged(_) => {}
            WindowEvent::DecorationsClick => {}
            _ => {}
        }
    }

    pub fn resume(
        &mut self,
        event_loop: &EventLoopWindowTarget<UserWindowEvent>,
        proxy: &EventLoopProxy<UserWindowEvent>,
        rt: &tokio::runtime::Runtime,
    ) {
        let window_builder = || {
            let window = WindowBuilder::new()
                .with_inner_size(LogicalSize {
                    width: 800,
                    height: 600,
                })
                .with_always_on_top(cfg!(debug_assertions))
                .build(event_loop)
                .unwrap();

            #[cfg(target_os = "windows")]
            {
                build_menu().init_for_hwnd(window.hwnd());
            }
            #[cfg(target_os = "linux")]
            {
                build_menu().init_for_gtk_window(window.gtk_window(), window.default_vbox());
            }

            // !TODO - this may not be the right way to do this, but it's a start
            // #[cfg(target_os = "macos")]
            // {
            //     menu_bar.init_for_nsapp();
            //     build_menu().set_as_windows_menu_for_nsapp();
            // }

            let size: tao::dpi::PhysicalSize<u32> = window.inner_size();
            let mut viewport = Viewport::new((size.width, size.height));
            viewport.set_hidpi_scale(window.scale_factor() as _);

            return (Arc::from(window), viewport);
        };

        rt.block_on(self.renderer.resume(window_builder));

        let RenderState::Active(state) = &self.renderer.render_state else {
            panic!("Renderer failed to resume");
        };

        self.waker = Some(crate::waker::tao_waker(&proxy, state.window.id()));
        self.renderer.render(&mut self.scene);
    }

    pub fn suspend(&mut self) {
        self.waker = None;
        self.renderer.suspend();
    }
}

fn build_menu() -> Menu {
    let mut menu = Menu::new();

    // Build the about section
    let mut about = Submenu::new("About", true);

    about.append_items(&[
        &PredefinedMenuItem::about("Dioxus".into(), Option::from(AboutMetadata::default())),
        &MenuItem::with_id(MenuId::new("dev.show_layout"), "Show layout", true, None),
    ]);

    menu.append(&about).unwrap();

    menu
}
