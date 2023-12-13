use dioxus::core::{Component, Mutations, VirtualDom};
use slab::Slab;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
    window::WindowBuilder,
};
use vello::{
    peniko::Color,
    util::{RenderContext, RenderSurface},
    RenderParams, Scene, SceneBuilder,
};
use vello::{Renderer as VelloRenderer, RendererOptions};

mod layout;
pub use layout::*;

mod render;
pub use render::*;

mod text;
pub use text::TextContext;

pub mod node;

pub mod style;

mod simple;

pub async fn render(app: Component) {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let mut dom = VirtualDom::new(app);

    let mutations = dom.rebuild();

    let mut application = RealDom::new(&window).await;
    application.apply(mutations);

    event_loop.run(move |event, _, control_flow| {
        // ControlFlow::Wait pauses the event loop if no events are available to process.
        // This is ideal for non-game applications that only update in response to user
        // input, and uses significantly less power/CPU time than ControlFlow::Poll.
        *control_flow = ControlFlow::Wait;

        // application.send_event(&event);

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

                // if !appliction.clean().is_empty() {
                application.render();
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
                // appliction.set_size(physical_size);
            }
            _ => (),
        }
    });
}

struct RealDom {
    nodes: Slab<Node>,
    render_context: RenderContext,
    surface: RenderSurface,
    wgpu_renderer: VelloRenderer,
    text_context: TextContext,
    layout: TaffyLayout,
}

struct Node {}

impl RealDom {
    async fn new(window: &Window) -> Self {
        let mut render_context = RenderContext::new().unwrap();
        let size = window.inner_size();
        let surface = render_context
            .create_surface(window, size.width, size.height)
            .await
            .expect("Error creating surface");
        let wgpu_renderer = VelloRenderer::new(
            &render_context.devices[surface.dev_id].device,
            &RendererOptions {
                surface_format: Some(surface.config.format),
                timestamp_period: render_context.devices[surface.dev_id]
                    .queue
                    .get_timestamp_period(),
            },
        )
        .unwrap();

        let text_context = TextContext::default();

        Self {
            nodes: Slab::new(),
            text_context,
            render_context,
            wgpu_renderer,
            surface,
            layout: TaffyLayout::default(),
            // event_handler,
        }
    }

    /// Currently just one off
    fn apply(&mut self, mutations: Mutations) {
        for template in mutations.templates {
            // build templates
        }

        for mutation in mutations.edits {
            // apply edits
        }
    }

    fn render(&mut self) {
        let mut scene = Scene::new();
        let mut builder = SceneBuilder::for_scene(&mut scene);

        // draw some squares

        // self.dom.render(&mut self.text_context, &mut builder);

        let surface_texture = self
            .surface
            .surface
            .get_current_texture()
            .expect("failed to get surface texture");
        let device = &self.render_context.devices[self.surface.dev_id];
        self.wgpu_renderer
            .render_to_surface(
                &device.device,
                &device.queue,
                &scene,
                &surface_texture,
                &RenderParams {
                    base_color: Color::WHITE,
                    width: self.surface.config.width,
                    height: self.surface.config.height,
                },
            )
            .expect("failed to render to surface");
        surface_texture.present();
        device.device.poll(wgpu::Maintain::Wait);
    }

    fn set_size(&mut self) {}
}
