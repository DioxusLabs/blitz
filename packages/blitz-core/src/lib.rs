use std::{
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
};

use application::ApplicationState;
use dioxus_native_core::prelude::*;

use futures_util::Future;
use taffy::Taffy;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

pub use crate::events::EventData;

mod application;
mod events;
mod focus;
mod image;
mod layout;
mod mouse;
mod prevent_default;
mod render;
mod style;
mod text;
mod util;

type TaoEvent<'a> = Event<'a, Redraw>;

#[derive(Debug)]
pub struct Redraw;

#[derive(Default)]
pub struct Config;

pub async fn render<R: Driver>(
    spawn_renderer: impl FnOnce(&Arc<RwLock<RealDom>>, &Arc<Mutex<Taffy>>) -> R + Send + 'static,
    _cfg: Config,
) {
    let event_loop = EventLoop::with_user_event();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let mut appliction =
        ApplicationState::new(spawn_renderer, &window, event_loop.create_proxy()).await;
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

pub trait Driver {
    fn update(&mut self, root: NodeMut);
    fn handle_event(&mut self, node: NodeMut, event: &str, value: Arc<EventData>, bubbles: bool);
    fn poll_async(&mut self) -> Pin<Box<dyn Future<Output = ()> + '_>>;
}
