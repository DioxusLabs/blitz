mod waker;
mod window;

use crate::waker::{EventData, UserWindowEvent};
use dioxus::prelude::*;
use std::collections::HashMap;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};
use muda::{MenuEvent, MenuId};
use tao::event_loop::EventLoopBuilder;
use tao::window::WindowId;
use blitz::RenderState;

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

    let _guard = rt.enter();

    // Build an event loop for the application
    let event_loop = EventLoopBuilder::<UserWindowEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Multiwindow ftw
    let mut windows : HashMap<WindowId, window::View> = HashMap::new();
    let mut pending_windows = Vec::new();
    let window = crate::window::View::new(&event_loop, app, props, &cfg);
    pending_windows.push(window);
    let menu_channel = MenuEvent::receiver();

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let mut initial = true;

    event_loop.run(move |event, event_loop, control_flow| {
        *control_flow = ControlFlow::Wait;

        let mut on_resume = || {
            for (_, view) in windows.iter_mut() {
                view.resume(&event_loop, &proxy, &rt);
            }

            for view in pending_windows.iter_mut() {
                view.resume(&event_loop, &proxy, &rt);
            }

            for window in pending_windows.drain(..) {
                let RenderState::Active(state) = &window.renderer.render_state else { continue };
                windows.insert(state.window.id(), window);
            }

            *control_flow = ControlFlow::Poll;
        };

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        if initial {
            on_resume();
            initial = false;
        }

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
                    view.request_redraw();
                }
            }

            Event::Suspended => {
                for (_, view) in windows.iter_mut() {
                    view.suspend();
                }
                *control_flow = ControlFlow::Wait;
            }

            Event::Resumed => on_resume(),

            Event::WindowEvent {
                window_id, event, ..
            } => {
                windows.get_mut(&window_id).map(|window| {
                    window.handle_window_event(event);
                });
            }

            _ => (),
        }

        if let Ok(event) = menu_channel.try_recv() {
            if event.id == MenuId::new("dev.show_layout") {
                for (_, view) in windows.iter_mut() {
                    view.renderer.devtools.show_layout = !view.renderer.devtools.show_layout;
                    view.request_redraw();
                }
            }
        }
    });
}
