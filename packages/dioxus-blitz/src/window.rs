use super::Config;
use crate::waker::UserWindowEvent;
use blitz::{Renderer, Viewport};
use blitz_dom::Document;
use dioxus::core::{Component, VirtualDom};
use futures_util::{pin_mut, FutureExt};
use std::task::Waker;
use tao::{
    event::WindowEvent,
    event_loop::EventLoop,
    keyboard::KeyCode,
    menu::{AboutMetadata, MenuBar, MenuId, MenuItemAttributes},
    window::{Window, WindowBuilder},
};
use vello::Scene;

pub(crate) struct View {
    pub(crate) window: Window,
    pub(crate) vdom: VirtualDom,
    pub(crate) renderer: Renderer,
    pub(crate) scene: Scene,
    pub(crate) waker: Waker,
}

impl View {
    pub(crate) fn new<P: 'static>(
        event_loop: &EventLoop<UserWindowEvent>,
        app: Component<P>,
        props: P,
        cfg: &Config,
        rt: &tokio::runtime::Runtime,
    ) -> Self {
        // By default we're drawing a single window
        // Set up the blitz drawing system
        // todo: this won't work on ios - blitz creation has to be deferred until the event loop as started
        let window = WindowBuilder::new()
            .with_always_on_top(cfg!(debug_assertions))
            .with_menu(build_menu())
            .build(&event_loop)
            .unwrap();

        let waker = crate::waker::tao_waker(&event_loop.create_proxy(), window.id());

        // Spin up the virtualdom
        // We're going to need to hit it with a special waker
        let mut vdom = VirtualDom::new_with_props(app, props);
        _ = vdom.rebuild();
        let markup = dioxus_ssr::render(&vdom);

        let size: tao::dpi::PhysicalSize<u32> = window.inner_size();
        let mut viewport = Viewport::new((size.width, size.height));
        viewport.set_hidpi_scale(window.scale_factor() as _);

        let device = viewport.make_device();

        let mut dom = Document::new(device);

        // Include the default stylesheet
        // todo: should this be done in blitz itself?
        dom.add_stylesheet(include_str!("./default.css"));

        // add default styles, resolve layout and styles
        for ss in &cfg.stylesheets {
            dom.add_stylesheet(&ss);
        }

        dom.write(markup);

        let mut renderer = rt.block_on(Renderer::from_window(&window, dom, viewport));
        let mut scene = Scene::new();

        renderer.dom.resolve();
        renderer.render(&mut scene);

        Self {
            window,
            vdom,
            renderer,
            scene,
            waker,
        }
    }

    pub(crate) fn poll(&mut self) {
        let mut cx = std::task::Context::from_waker(&self.waker);

        loop {
            {
                let fut = self.vdom.wait_for_work();
                pin_mut!(fut);

                match fut.poll_unpin(&mut cx) {
                    std::task::Poll::Ready(_) => {}
                    std::task::Poll::Pending => break,
                }
            }

            let edits = self.vdom.render_immediate();

            // apply the mutations to the actual dom

            // send_edits(view.dom.render_immediate(), &view.desktop_context.webview);
        }
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
                self.window.request_redraw();
            }

            // todo: if there's an active text input, we want to direct input towards it and translate system emi text
            WindowEvent::KeyboardInput { event, .. } => {
                dbg!(&event);

                match event.physical_key {
                    KeyCode::ArrowUp => {
                        self.renderer.zoom(0.005);
                        self.window.request_redraw();
                    }
                    KeyCode::ArrowDown => {
                        self.renderer.zoom(-0.005);
                        self.window.request_redraw();
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
}

fn build_menu() -> MenuBar {
    let mut menu = MenuBar::new();

    // Build the about section
    let mut about = MenuBar::new();
    about.add_native_item(tao::menu::MenuItem::About(
        "Dioxus".into(),
        AboutMetadata::default(),
    ));
    about.add_item(MenuItemAttributes::new("Show layout").with_id(MenuId::new("dev.show_layout")));

    menu.add_submenu("about", true, about);

    menu
}
