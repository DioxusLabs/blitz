use crate::{dom::styling::RealDom, viewport::Viewport};
use crate::{
    dom::Document,
    waker::{EventData, UserWindowEvent},
};
use dioxus::prelude::*;
use futures_util::{pin_mut, FutureExt};
use std::{collections::HashMap, task::Waker};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopProxy},
    window::{Window, WindowBuilder, WindowId},
};
use vello::Scene;

#[derive(Default)]
pub struct Config {
    pub stylesheets: Vec<String>,
}

/// Launch an interactive HTML/CSS renderer driven by the Dioxus virtualdom
pub fn launch(app: Component<()>) {
    launch_cfg(app, Config::default())
}

pub fn launch_cfg(app: Component<()>, cfg: Config) {
    launch_cfg_with_props(app, (), cfg)
}

// todo: props shouldn't have the clone bound - should try and match dioxus-desktop behavior
pub fn launch_cfg_with_props<Props: 'static + Send + Clone>(
    app: Component<Props>,
    props: Props,
    cfg: Config,
) {
    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let _ = rt.enter();

    // Build an event loop for the application
    let event_loop = EventLoop::<UserWindowEvent>::with_user_event();

    // Multiwindow ftw
    let mut windows = HashMap::new();

    // All apps start with a single window
    let window = View::new(&event_loop, app, props, &cfg, &rt);
    windows.insert(window.window.id(), window);

    let proxy = event_loop.create_proxy();

    event_loop.run(move |event, _target, control_flow| {
        // ControlFlow::Wait pauses the event loop if no events are available to process.
        // This is ideal for non-game applications that only update in response to user
        // input, and uses significantly less power/CPU time than ControlFlow::Poll.
        *control_flow = ControlFlow::Wait;

        // appliction.send_event(&event);

        match event {
            // Exit the app when close is request
            // Not always necessary
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,

            // Nothing else to do, try redrawing?
            Event::MainEventsCleared => {
                // We might not actually want this if nothing has changed
                // window.request_redraw()
            }

            Event::UserEvent(UserWindowEvent(EventData::Poll, id)) => {
                windows.get_mut(&id).map(|view| view.poll());
            }

            Event::NewEvents(cause) => {
                for id in windows.keys() {
                    _ = proxy.send_event(UserWindowEvent(EventData::Poll, *id));
                }
            }

            Event::RedrawRequested(window_id) => {
                // Redraw the application.
                //
                // It's preferable for applications that do not render continuously to render in
                // this event rather than in MainEventsCleared, since rendering in here allows
                // the program to gracefully handle redraws requested by the OS.

                windows.get_mut(&window_id).map(|window| {
                    window.document.resolve_layout();
                    window.document.render(&mut window.scene);
                });
            }

            Event::UserEvent(_redraw) => {
                for (_, view) in windows.iter() {
                    view.window.request_redraw();
                }
            }

            Event::WindowEvent {
                window_id, event, ..
            } => {
                //
                match event {
                    WindowEvent::MouseInput {
                        device_id,
                        state,
                        button,
                        modifiers,
                    } => {}
                    WindowEvent::Resized(_) => {}
                    WindowEvent::Moved(_) => {}
                    WindowEvent::CloseRequested => {}
                    WindowEvent::Destroyed => {}
                    WindowEvent::DroppedFile(_) => {}
                    WindowEvent::HoveredFile(_) => {}
                    WindowEvent::HoveredFileCancelled => {}
                    WindowEvent::ReceivedImeText(_) => {}
                    WindowEvent::Focused(_) => {}
                    WindowEvent::KeyboardInput { .. } => {}
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

            Event::WindowEvent {
                event: WindowEvent::Resized(physical_size),
                window_id,
                ..
            } => {
                println!("resizing!");
                windows.get_mut(&window_id).map(|window| {
                    window.document.set_size(physical_size);
                    window.window.request_redraw();
                });
            }
            _ => (),
        }
    });
}

struct View {
    window: Window,
    vdom: VirtualDom,
    document: Document,
    scene: Scene,
    waker: Waker,
}

impl View {
    fn new<P: 'static>(
        event_loop: &EventLoop<UserWindowEvent>,
        app: Component<P>,
        props: P,
        cfg: &Config,
        rt: &tokio::runtime::Runtime,
    ) -> Self {
        // By default we're drawing a single window
        let window = WindowBuilder::new().build(&event_loop).unwrap();

        // Spin up the virtualdom
        // We're going to need to hit it with a special waker
        let mut vdom = VirtualDom::new_with_props(app, props);
        _ = vdom.rebuild();

        let markup = dioxus_ssr::render(&vdom);

        let waker = crate::waker::tao_waker(&event_loop.create_proxy(), window.id());

        // Set up the blitz drawing system
        // todo: this won't work on ios - blitz creation has to be deferred until the event loop as started
        let dom = RealDom::new(markup);

        let size = window.inner_size();
        let mut viewport = Viewport::new(size);
        viewport.hidpi_scale = dbg!(window.scale_factor()) as _;

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

    fn poll(&mut self) {
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
}
