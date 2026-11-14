//! Iced on its own iced_winit + iced_wgpu loop (as in iced's `integration` example), so
//! each phase can be timed: update = apply the op + `view()`; build = `UserInterface::build`
//! (widget tree diff against the previous frame's cache + layout), event handling and
//! `draw` into layers; raster_cpu = `Renderer::present` (prepare uploads, encode, submit);
//! acquire / present = swapchain.
//!
//! No `raster_gpu`: iced_wgpu creates and submits its own command encoder, so there is
//! nowhere to put timestamp queries around just its work.

use bench_core::scroll::{Block, BlockKind};
use bench_core::table::TableModel;
use bench_core::{Args, Driver, Op, Phase, Workload};
use iced_wgpu::graphics::{Shell, Viewport};
use iced_wgpu::{Engine, Renderer, wgpu};
use iced_widget::{button, column, keyed_column, row, scrollable, text};
use iced_winit::Clipboard;
use iced_winit::core::widget::operation::scrollable::{AbsoluteOffset, scroll_to};
use iced_winit::core::{Color, Element, Event, Font, Length, Pixels, Size, Theme, font};
use iced_winit::core::{mouse, renderer, window};
use iced_winit::runtime::user_interface::{self, UserInterface};
use iced_winit::winit;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};

const SCROLL_ID: &str = "document";

struct Gfx {
    window: Arc<winit::window::Window>,
    device: wgpu::Device,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    viewport: Viewport,
    clipboard: Clipboard,
    cache: user_interface::Cache,
}

struct App {
    args: Args,
    driver: Driver,
    gfx: Option<Gfx>,
    model: TableModel,
    doc: Vec<Block>,
}

type El<'a> = Element<'a, (), Theme, Renderer>;

fn view<'a>(workload: Workload, model: &'a TableModel, doc: &'a [Block]) -> El<'a> {
    match workload {
        Workload::Table => {
            let rows = keyed_column(model.rows.iter().map(|r| {
                let colour = if model.selected == Some(r.id) {
                    Color::from_rgb(0.8, 0.1, 0.1)
                } else {
                    Color::BLACK
                };
                let el: El<'a> = row![
                    text(r.id).width(60),
                    text(r.label.as_str()).color(colour).width(Length::Fill),
                    button(text("x")),
                ]
                .spacing(8)
                .into();
                (r.id, el)
            }));
            scrollable(rows)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
        Workload::Scroll => {
            let blocks = column(doc.iter().map(|b| {
                let font = match b.kind {
                    BlockKind::Heading => Font {
                        weight: font::Weight::Bold,
                        ..Font::DEFAULT
                    },
                    BlockKind::Paragraph => Font::DEFAULT,
                    BlockKind::Emphasis => Font {
                        style: font::Style::Italic,
                        ..Font::DEFAULT
                    },
                    BlockKind::Code => Font::MONOSPACE,
                };
                text(b.text.as_str())
                    .size(b.kind.font_size())
                    .font(font)
                    .into()
            }))
            .spacing(8)
            .width(Length::Fill);
            scrollable(blocks)
                .id(SCROLL_ID)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    }
}

impl App {
    fn init(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = winit::window::Window::default_attributes()
            .with_title("gui-bench: iced")
            .with_resizable(false)
            .with_inner_size(winit::dpi::LogicalSize::new(
                bench_core::WINDOW_WIDTH,
                bench_core::WINDOW_HEIGHT,
            ));
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        let size = window.inner_size();
        let viewport = Viewport::with_physical_size(
            Size::new(size.width, size.height),
            window.scale_factor() as f32,
        );
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no wgpu adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut config = surface
            .get_default_config(&adapter, size.width, size.height)
            .unwrap();
        config.present_mode = if self.args.vsync {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        surface.configure(&device, &config);
        let engine = Engine::new(
            &adapter,
            device.clone(),
            queue,
            config.format,
            None,
            Shell::headless(),
        );
        let renderer = Renderer::new(engine, Font::default(), Pixels::from(16));
        let info = adapter.get_info();
        self.driver.meta(&[
            ("renderer", "iced_wgpu".into()),
            ("adapter", format!("{} ({:?})", info.name, info.backend)),
            ("gpu_timing", "not exposed".into()),
            ("scale_factor", window.scale_factor().to_string()),
            ("present_mode", format!("{:?}", config.present_mode)),
        ]);
        window.request_redraw();
        self.gfx = Some(Gfx {
            clipboard: Clipboard::connect(window.clone()),
            window,
            device,
            surface,
            config,
            renderer,
            viewport,
            cache: user_interface::Cache::new(),
        });
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.driver.is_done() {
            event_loop.exit();
            return;
        }
        let gfx = self.gfx.as_mut().unwrap();
        let start = Instant::now();
        let frame = match gfx.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Outdated) => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                gfx.window.request_redraw();
                return;
            }
            Err(wgpu::SurfaceError::Timeout) => {
                gfx.window.request_redraw();
                return;
            }
            Err(e) => panic!("couldn't acquire a surface texture: {e}"),
        };
        let op = self.driver.begin_frame_at(start).unwrap();
        self.driver.lap(Phase::Acquire);

        let mut scroll_y = None;
        match op {
            Op::Table(op) => self.model.apply(op),
            Op::Scroll { y } => scroll_y = Some(y),
        }
        let element = view(self.args.workload, &self.model, &self.doc);
        self.driver.lap(Phase::Update);

        let mut interface = UserInterface::build(
            element,
            gfx.viewport.logical_size(),
            std::mem::take(&mut gfx.cache),
            &mut gfx.renderer,
        );
        if let Some(y) = scroll_y {
            let mut op = scroll_to(
                SCROLL_ID.into(),
                AbsoluteOffset {
                    x: None,
                    y: Some(y),
                },
            );
            interface.operate(&gfx.renderer, &mut op);
        }
        let cursor = mouse::Cursor::Unavailable;
        let _ = interface.update(
            &[Event::Window(
                window::Event::RedrawRequested(Instant::now()),
            )],
            cursor,
            &mut gfx.renderer,
            &mut gfx.clipboard,
            &mut Vec::new(),
        );
        interface.draw(
            &mut gfx.renderer,
            &Theme::Light,
            &renderer::Style::default(),
            cursor,
        );
        gfx.cache = interface.into_cache();
        self.driver.lap(Phase::Build);

        let view = frame.texture.create_view(&Default::default());
        gfx.renderer.present(
            Some(Color::WHITE),
            frame.texture.format(),
            &view,
            &gfx.viewport,
        );
        self.driver.lap(Phase::RasterCpu);

        gfx.window.pre_present_notify();
        frame.present();
        self.driver.lap(Phase::Present);
        self.driver.end_frame();
        gfx.window.request_redraw();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_none() {
            self.init(event_loop);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }
}

fn main() {
    let args = Args::parse();
    let driver = Driver::new("iced", &args);
    let mut app = App {
        args,
        driver,
        gfx: None,
        model: TableModel::default(),
        doc: bench_core::scroll::document(),
    };
    EventLoop::new().unwrap().run_app(&mut app).unwrap();
}
