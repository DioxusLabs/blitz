mod waker;
mod window;

use crate::waker::{EventData, UserWindowEvent};
use dioxus::prelude::*;
use std::collections::HashMap;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};

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
    // Build an event loop for the application
    let event_loop = EventLoop::<UserWindowEvent>::with_user_event();

    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let _guard = rt.enter();

    // Multiwindow ftw
    let mut windows = HashMap::new();

    // All apps start with a single window
    let window = crate::window::View::new(&event_loop, app, props, &cfg, &rt);
    windows.insert(window.window.id(), window);

    let proxy = event_loop.create_proxy();

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            // Exit the app when close is request
            // Not always necessary
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,

            // Nothing else to do, try redrawing?
            Event::MainEventsCleared => {}

            Event::UserEvent(UserWindowEvent(EventData::Poll, id)) => {
                windows.get_mut(&id).map(|view| view.poll());
            }

            Event::NewEvents(_) => {
                for id in windows.keys() {
                    _ = proxy.send_event(UserWindowEvent(EventData::Poll, *id));
                }
            }

            Event::RedrawRequested(window_id) => {
                windows.get_mut(&window_id).map(|window| {
                    window.renderer.dom.resolve();
                    window.renderer.render(&mut window.scene);
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
                windows.get_mut(&window_id).map(|window| {
                    window.handle_window_event(event);
                });
            }

            _ => (),
        }
    });
}
