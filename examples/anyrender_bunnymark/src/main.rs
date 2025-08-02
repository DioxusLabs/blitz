use anyrender::{PaintScene, WindowRenderer};
use anyrender_vello::VelloWindowRenderer;
use anyrender_vello_cpu::VelloCpuWindowRenderer;
use bunny::BunnyManager;
use kurbo::{Affine, Circle, Point, Rect, Stroke};
use peniko::{Color, Fill};
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, NamedKey, SmolStr},
    window::{Window, WindowId},
};

mod bunny;

const SKY_BLUE: Color = Color::from_rgb8(135, 206, 235);

struct App {
    render_state: RenderState,
    bunny_manager: BunnyManager,
    logical_width: u32,
    logical_height: u32,
    scale_factor: f64,
}

enum Renderer {
    Gpu(Box<VelloWindowRenderer>),
    Cpu(Box<VelloCpuWindowRenderer>),
}

impl Renderer {
    fn is_active(&self) -> bool {
        match self {
            Renderer::Gpu(r) => r.is_active(),
            Renderer::Cpu(r) => r.is_active(),
        }
    }

    fn set_size(&mut self, w: u32, h: u32) {
        match self {
            Renderer::Gpu(r) => r.set_size(w, h),
            Renderer::Cpu(r) => r.set_size(w, h),
        }
    }
}

enum RenderState {
    Active {
        window: Arc<Window>,
        renderer: Renderer,
    },
    Suspended(Option<Arc<Window>>),
}

impl App {
    fn request_redraw(&mut self) {
        let window = match &self.render_state {
            RenderState::Active { window, renderer } => {
                if renderer.is_active() {
                    Some(window)
                } else {
                    None
                }
            }
            RenderState::Suspended(_) => None,
        };

        if let Some(window) = window {
            window.request_redraw();
        }
    }

    fn draw_scene<T: PaintScene>(
        scene: &mut T,
        width: u32,
        height: u32,
        scale_factor: f64,
        bunny_manager: &BunnyManager,
        color: Color,
    ) {
        // Draw background
        scene.fill(
            Fill::NonZero,
            Affine::scale(scale_factor),
            SKY_BLUE,
            None,
            &Rect::new(0.0, 0.0, width as f64, height as f64),
        );

        // Draw small circle indicating renderer in use
        scene.stroke(
            &Stroke::new(2.0),
            Affine::scale(scale_factor),
            Color::BLACK,
            None,
            &Rect::new(5.0, 5.0, 35.0, 35.0),
        );
        scene.fill(
            Fill::NonZero,
            Affine::scale(scale_factor),
            color,
            None,
            &Circle::new(Point::new(20.0, 20.0), 10.0),
        );

        // Draw bunnies
        bunny_manager.draw(scene, scale_factor);
    }

    fn set_backend<R: WindowRenderer>(
        &mut self,
        mut renderer: R,
        event_loop: &ActiveEventLoop,
        f: impl FnOnce(R) -> Renderer,
    ) {
        let mut window = match &self.render_state {
            RenderState::Active { window, .. } => Some(window.clone()),
            RenderState::Suspended(cached_window) => cached_window.clone(),
        };
        let window = window.take().unwrap_or_else(|| {
            let attr = Window::default_attributes()
                .with_inner_size(winit::dpi::LogicalSize::new(
                    self.logical_width,
                    self.logical_height,
                ))
                .with_resizable(true)
                .with_title("anyrender + winit demo")
                .with_visible(true)
                .with_active(true);
            Arc::new(event_loop.create_window(attr).unwrap())
        });
        self.scale_factor = window.scale_factor();

        let physical_size = window.inner_size();
        renderer.resume(window.clone(), physical_size.width, physical_size.height);
        self.render_state = RenderState::Active {
            window,
            renderer: f(renderer),
        };
        self.request_redraw();
    }
}

impl ApplicationHandler for App {
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let RenderState::Active { window, .. } = &self.render_state {
            self.render_state = RenderState::Suspended(Some(window.clone()));
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.set_backend(VelloCpuWindowRenderer::new(), event_loop, |r| {
            Renderer::Cpu(Box::new(r))
        });
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let RenderState::Active { window, renderer } = &mut self.render_state else {
            return;
        };

        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(physical_size) => {
                let logical_size = physical_size.to_logical(self.scale_factor);
                self.logical_width = logical_size.width;
                self.logical_height = logical_size.height;
                renderer.set_size(physical_size.width, physical_size.height);
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor;
                if let RenderState::Active { window, renderer } = &mut self.render_state {
                    let physical_size = window.inner_size();
                    let logical_size = physical_size.to_logical(scale_factor);
                    self.logical_width = logical_size.width;
                    self.logical_height = logical_size.height;
                    renderer.set_size(physical_size.width, physical_size.height);
                };
            }
            WindowEvent::RedrawRequested => {
                self.bunny_manager
                    .update(self.logical_width as f64, self.logical_height as f64);
                let renderer_name = match renderer {
                    Renderer::Gpu(_) => "vello",
                    Renderer::Cpu(_) => "vello_cpu",
                };
                print!(
                    "[{}] [{} bunnies] ",
                    renderer_name,
                    self.bunny_manager.count(),
                );
                match renderer {
                    Renderer::Gpu(r) => r.render(|scene_painter| {
                        App::draw_scene(
                            scene_painter,
                            self.logical_width,
                            self.logical_height,
                            self.scale_factor,
                            &self.bunny_manager,
                            Color::from_rgb8(255, 0, 0),
                        );
                    }),
                    Renderer::Cpu(r) => r.render(|scene_painter| {
                        App::draw_scene(
                            scene_painter,
                            self.logical_width,
                            self.logical_height,
                            self.scale_factor,
                            &self.bunny_manager,
                            Color::from_rgb8(0, 255, 0),
                        );
                    }),
                }
                window.request_redraw();
            }
            WindowEvent::MouseInput { state, .. } => {
                if state.is_pressed() {
                    self.bunny_manager.add_bunnies(100);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                if logical_key == Key::Named(NamedKey::Space) {
                    match renderer {
                        Renderer::Cpu(_) => {
                            self.set_backend(VelloWindowRenderer::new(), event_loop, |r| {
                                Renderer::Gpu(Box::new(r))
                            });
                        }
                        Renderer::Gpu(_) => {
                            self.set_backend(VelloCpuWindowRenderer::new(), event_loop, |r| {
                                Renderer::Cpu(Box::new(r))
                            });
                        }
                    }
                } else if logical_key == Key::Character(SmolStr::new("r")) {
                    self.bunny_manager.clear_bunnies();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let mut app = App {
        render_state: RenderState::Suspended(None),
        bunny_manager: BunnyManager::new(1024.0, 1024.0),
        logical_width: 1024,
        logical_height: 1024,
        scale_factor: 1.0,
    };

    let event_loop = EventLoop::new().unwrap();
    event_loop
        .run_app(&mut app)
        .expect("Couldn't run event loop");
}
