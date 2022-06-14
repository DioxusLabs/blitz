use application::ApplicationState;
use dioxus::core as dioxus_core;
use dioxus::native_core as dioxus_native_core;
use dioxus::{
    native_core::{
        real_dom::{Node, RealDom},
        state::*,
    },
    native_core_macro::State,
    prelude::*,
};
use layout::StretchLayout;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

mod application;
mod layout;
mod render;
mod style;
mod util;

#[derive(Clone, PartialEq, Default, State)]
struct BlitzNodeState {
    #[child_dep_state(layout, Rc<RefCell<Stretch>>)]
    layout: StretchLayout,
    #[state]
    style: style::Style,
}

type Dom = RealDom<BlitzNodeState>;
type DomNode = Node<BlitzNodeState>;

#[derive(Debug)]
pub struct Redraw;

#[derive(Default)]
pub struct Config;

pub fn launch(root: Component<()>) {
    launch_cfg(root, Config::default())
}

pub fn launch_cfg(root: Component<()>, _cfg: Config) {
    let event_loop = EventLoop::with_user_event();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let mut appliction = ApplicationState::new(root, &window, event_loop.create_proxy());

    event_loop.run(move |event, _, control_flow| {
        // ControlFlow::Wait pauses the event loop if no events are available to process.
        // This is ideal for non-game applications that only update in response to user
        // input, and uses significantly less power/CPU time than ControlFlow::Poll.
        *control_flow = ControlFlow::Wait;

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
