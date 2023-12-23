use super::Config;
use crate::dom::styling::RealDom;
use crate::dom::Document;
use crate::viewport::Viewport;
use crate::waker::UserWindowEvent;
use dioxus::core::{Component, VirtualDom};
use futures_util::{pin_mut, FutureExt};
use std::task::Waker;
use tao::event::WindowEvent;
use tao::event_loop::EventLoop;
use tao::keyboard::KeyCode;
use tao::window::Window;
use tao::window::WindowBuilder;
use vello::Scene;

pub(crate) struct View {
    pub(crate) window: Window,
    pub(crate) vdom: VirtualDom,
    pub(crate) document: Document,
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

        let window = WindowBuilder::new().build(&event_loop).unwrap();

        // Spin up the virtualdom
        // We're going to need to hit it with a special waker
        let mut vdom = VirtualDom::new_with_props(app, props);
        _ = vdom.rebuild();

        let markup = dioxus_ssr::render(&vdom);

        let waker = crate::waker::tao_waker(&event_loop.create_proxy(), window.id());

        let dom = RealDom::new(markup);

        let size = window.inner_size();
        let mut viewport = Viewport::new(size);
        viewport.set_hidpi_scale(window.scale_factor() as _);

        let mut document = rt.block_on(Document::from_window(&window, dom, viewport));
        let mut scene = Scene::new();

        // add default styles, resolve layout and styles
        for ss in &cfg.stylesheets {
            document.add_stylesheet(&ss);
        }

        document.resolve();
        document.render(&mut scene);

        Self {
            window,
            vdom,
            document,
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
                self.document.set_size(physical_size);
                self.window.request_redraw();
            }

            // todo: if there's an active text input, we want to direct input towards it and translate system emi text
            WindowEvent::KeyboardInput { event, .. } => {
                //
                use tao::keyboard::KeyCode;
                dbg!(&event);

                match event.physical_key {
                    KeyCode::ArrowUp => {
                        *self.document.viewport.zoom_mut() += 0.1;
                        self.window.request_redraw();
                    }
                    KeyCode::ArrowDown => {
                        *self.document.viewport.zoom_mut() -= 0.1;
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
