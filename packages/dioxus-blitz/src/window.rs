use std::sync::Arc;
use super::Config;
use crate::waker::UserWindowEvent;
use blitz::{Renderer, Viewport, RenderState};
use blitz_dom::Document;
use dioxus::dioxus_core::{Component, ComponentFunction, VirtualDom};
use futures_util::{pin_mut, FutureExt};
use std::task::Waker;
use dioxus::html::view;
use dioxus::prelude::Element;
use tao::{
    event::WindowEvent,
    event_loop::EventLoop,
    keyboard::KeyCode,
    window::{Window, WindowBuilder},
};
use muda::{AboutMetadata, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use style::media_queries::Device;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
#[cfg(target_os = "windows")]
use tao::platform::windows::WindowExtWindows;
use vello::Scene;


pub(crate) struct View<'s> {
    pub(crate) renderer: Renderer<'s, Window>,
    pub(crate) vdom: VirtualDom,
    pub(crate) scene: Scene,
    pub(crate) waker: Option<Waker>,
}

impl<'a> View<'a> {
    pub(crate) fn new<P: 'static + Clone, M: 'static>(
        event_loop: &EventLoop<UserWindowEvent>,
        root: impl ComponentFunction<P, M>,
        props: P,
        cfg: &Config,
    ) -> Self {
        // Spin up the virtualdom
        // We're going to need to hit it with a special waker
        let mut vdom = VirtualDom::new_with_props(root, props);
        vdom.rebuild_in_place();
        let markup = dioxus_ssr::render(&vdom);
        let mut scene = Scene::new();

        let mut dom = Document::new(Viewport::new((0,0)).make_device());

        // Include the default stylesheet
        // todo: should this be done in blitz itself?
        dom.add_stylesheet(include_str!("./default.css"));

        // add default styles, resolve layout and styles
        for ss in &cfg.stylesheets {
            dom.add_stylesheet(&ss);
        }

        dom.write(markup);

        // let size: tao::dpi::PhysicalSize<u32> = window.inner_size();
        // let mut viewport = Viewport::new((size.width, size.height));
        // viewport.set_hidpi_scale(window.scale_factor() as _);

        // let device = viewport.make_device();
        // self.dom.set_stylist_device(device);

        let mut renderer = Renderer::new(dom);


        Self {
            renderer,
            vdom,
            scene,
            waker: None
        }
    }

    pub(crate) fn poll(&mut self) {
        match &self.waker {
            None => {},
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
            } => {}

            WindowEvent::Resized(physical_size) => {
                self.renderer
                    .set_size((physical_size.width, physical_size.height));
                self.request_redraw();
            }

            // todo: if there's an active text input, we want to direct input towards it and translate system emi text
            WindowEvent::KeyboardInput { event, .. } => {
                dbg!(&event);

                match event.physical_key {
                    KeyCode::ArrowUp => {
                        self.renderer.zoom(0.005);
                        self.request_redraw();
                    }
                    KeyCode::ArrowDown => {
                        self.renderer.zoom(-0.005);
                        self.request_redraw();
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
            WindowEvent::ModifiersChanged(_) => {}
            WindowEvent::CursorMoved {
                device_id,
                position,
                modifiers,
            } => {}
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
        &PredefinedMenuItem::about(
            "Dioxus".into(),
            Option::from(AboutMetadata::default()),
        ),
        &MenuItem::with_id(
            MenuId::new("dev.show_layout"),
            "Show layout",
            true,
            None,
        )
    ]);

    menu.append(&about).unwrap();

    menu
}
