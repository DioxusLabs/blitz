use crate::node::BlitzNodeState;
use application::ApplicationState;
use dioxus::prelude::*;
use dioxus_native_core::{node::Node, real_dom::RealDom};

use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

mod application;
mod events;
mod focus;
mod layout;
mod mouse;
mod node;
mod render;
mod style;
mod text;
mod util;

type Dom = RealDom<BlitzNodeState>;
type DomNode = Node<BlitzNodeState>;
type TaoEvent<'a> = Event<'a, Redraw>;

#[derive(Debug)]
pub struct Redraw;

#[derive(Default)]
pub struct Config;

pub async fn launch(root: Component<()>) {
    launch_cfg(root, Config::default()).await
}

pub async fn launch_cfg(root: Component<()>, _cfg: Config) {
    let event_loop = EventLoop::with_user_event();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let mut appliction = ApplicationState::new(root, &window, event_loop.create_proxy()).await;
    appliction.render();

    event_loop.run(move |event, _, control_flow| {
        // ControlFlow::Wait pauses the event loop if no events are available to process.
        // This is ideal for non-game applications that only update in response to user
        // input, and uses significantly less power/CPU time than ControlFlow::Poll.
        *control_flow = ControlFlow::Wait;

        appliction.send_event(&event);

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            Event::MainEventsCleared => {
                // Application update code.

                // Queue a RedrawRequested event.
                //
                // You only need to call this if you've determined that you need to redraw, in
                // applications which do not always need to. Applications that redraw continuously
                // can just render here instead.
                window.request_redraw();
            }
            Event::RedrawRequested(_) => {
                // Redraw the application.
                //
                // It's preferable for applications that do not render continuously to render in
                // this event rather than in MainEventsCleared, since rendering in here allows
                // the program to gracefully handle redraws requested by the OS.

                if !appliction.clean().is_empty() {
                    appliction.render();
                }
            }
            Event::UserEvent(_redraw) => {
                window.request_redraw();
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(physical_size),
                window_id: _,
                ..
            } => {
                appliction.set_size(physical_size);
            }
            _ => (),
        }
    });
}
