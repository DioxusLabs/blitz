use std::{
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
};

use dioxus::prelude::*;
use futures_util::Future;
use taffy::TaffyTree;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{WindowBuilder, WindowId},
};

use crate::dom::styling::RealDom;
use crate::{
    dom::Document,
    waker::{EventData, UserWindowEvent},
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

pub fn launch_cfg_with_props<Props: 'static + Send>(
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

    // By default we're drawing a single window
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    // Spin up the virtualdom
    // We're going to need to hit it with a special waker
    let mut virtualdom = VirtualDom::new_with_props(app, props);
    _ = virtualdom.rebuild();
    let markup = dioxus_ssr::render(&virtualdom);

    // Set up the blitz drawing system
    // todo: this won't work on ios - blitz creation has to be deferred until the event loop as started
    let dom = RealDom::new(markup);
    let mut blitz = rt.block_on(Document::from_window(&window, dom));

    // add default styles, resolve layout and styles
    for ss in cfg.stylesheets {
        blitz.add_stylesheet(&ss);
    }

    blitz.resolve();
    blitz.render();

    event_loop.run(move |event, _, control_flow| {
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
                window.request_redraw()
            }

            Event::UserEvent(UserWindowEvent(EventData::Poll, id)) => {
                // todo: poll the virtualdom
            }

            Event::RedrawRequested(_) => {
                // Redraw the application.
                //
                // It's preferable for applications that do not render continuously to render in
                // this event rather than in MainEventsCleared, since rendering in here allows
                // the program to gracefully handle redraws requested by the OS.

                // if !appliction.clean().is_empty() {
                // blitz.render();
                // }
            }

            Event::UserEvent(_redraw) => {
                window.request_redraw();
            }

            Event::WindowEvent {
                event: WindowEvent::Resized(physical_size),
                window_id: _,
                ..
            } => {
                blitz.set_size(physical_size);
                window.request_redraw();
            }
            _ => (),
        }
    });
}
