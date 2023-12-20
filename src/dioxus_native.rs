use std::{
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
};

use dioxus::prelude::*;
use futures_util::Future;
use taffy::Taffy;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{WindowBuilder, WindowId},
};

use crate::{
    blitz::Document,
    waker::{EventData, UserWindowEvent},
};

#[derive(Default)]
pub struct Config;

/// Launch an interactive HTML/CSS renderer driven by the Dioxus virtualdom
pub fn launch(app: Component<()>) {
    launch_cfg(app, Config)
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

    // Set up the blitz drawing system
    // todo: this won't work on ios - blitz creation has to be deferred until the event loop as started
    let mut blitz = rt.block_on(Document::from_window(&window));

    // add default styles
    blitz.add_stylesheet(DEFAULT_STYLESHEET);

    // Spin up the virtualdom
    // We're going to need to hit it with a special waker
    let mut virtualdom = VirtualDom::new_with_props(app, props);

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
                blitz.render();
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

const DEFAULT_STYLESHEET: &str = r#"
        h1 {
            background-color: red;
        }

        h2 {
            background-color: green;
        }

        h3 {
            background-color: blue;
        }

        h4 {
            background-color: yellow;
        }

        .heading {
            padding: 5px;
            border-radius: 5px;
            border: 2px solid #73AD21;
        }

        div {
            margin: 35px;
        }
    "#;
