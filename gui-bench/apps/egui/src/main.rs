//! egui on its own winit + egui-wgpu loop, so each phase can be timed:
//! update = apply the op to the model; build = run the UI closure (immediate mode: widget
//! code, layout and shape generation are one pass) + tessellate; raster_cpu = texture and
//! buffer uploads, render pass encoding and submit; raster_gpu = the render pass on the
//! GPU; acquire / present = swapchain. A frame starts when the swapchain image is
//! acquired, then the op is applied.

use bench_core::scroll::{Block, BlockKind};
use bench_core::table::TableModel;
use bench_core::{Args, Driver, Op, Phase, Workload};
use egui::RichText;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

bench_core::define_gpu_timer!(
    wgpu,
    |device: &wgpu::Device| {
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    },
    |slice: wgpu::BufferSlice<'_>| slice.get_mapped_range().unwrap().to_vec()
);

struct Gfx {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: egui_wgpu::Renderer,
    timer: Option<GpuTimer>,
}

struct App {
    args: Args,
    driver: Driver,
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    ctx: egui::Context,
    state: Option<egui_winit::State>,
    model: TableModel,
    doc: Vec<Block>,
    scroll_y: f32,
}

impl App {
    fn init(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("gui-bench: egui")
            .with_resizable(false)
            .with_inner_size(winit::dpi::LogicalSize::new(
                bench_core::WINDOW_WIDTH,
                bench_core::WINDOW_HEIGHT,
            ));
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no wgpu adapter");
        let mut required_features = wgpu::Features::empty();
        if self.args.gpu_timing {
            required_features |= adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features,
            ..Default::default()
        }))
        .unwrap();
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width, size.height)
            .unwrap();
        config.present_mode = if self.args.vsync {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        surface.configure(&device, &config);
        let renderer = egui_wgpu::Renderer::new(&device, config.format, Default::default());
        let timer = GpuTimer::new(&device, &queue);
        let info = adapter.get_info();
        self.driver.meta(&[
            ("renderer", "egui-wgpu".into()),
            ("adapter", format!("{} ({:?})", info.name, info.backend)),
            ("gpu_timing", gpu_timing(&self.args, timer.is_some())),
            ("scale_factor", window.scale_factor().to_string()),
            ("present_mode", format!("{:?}", config.present_mode)),
        ]);
        self.state = Some(egui_winit::State::new(
            self.ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        ));
        self.gfx = Some(Gfx {
            surface,
            device,
            queue,
            config,
            renderer,
            timer,
        });
        window.request_redraw();
        self.window = Some(window);
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.driver.is_done() {
            event_loop.exit();
            return;
        }
        // Acquire first: if the window is occluded the frame is retried later, before
        // the op is applied.
        let start = Instant::now();
        let gfx = self.gfx.as_mut().unwrap();
        let frame = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                self.window.as_ref().unwrap().request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                std::thread::sleep(std::time::Duration::from_millis(5));
                self.window.as_ref().unwrap().request_redraw();
                return;
            }
            other => panic!("couldn't acquire a surface texture: {other:?}"),
        };
        let op = self.driver.begin_frame_at(start).unwrap();
        self.driver.lap(Phase::Acquire);
        match op {
            Op::Table(op) => self.model.apply(op),
            Op::Scroll { y } => self.scroll_y = y,
        }
        self.driver.lap(Phase::Update);

        let window = self.window.as_ref().unwrap();
        let state = self.state.as_mut().unwrap();
        let raw_input = state.take_egui_input(window);
        let ctx = self.ctx.clone();
        let (workload, model, doc, scroll_y) =
            (self.args.workload, &self.model, &self.doc, self.scroll_y);
        let output = ctx.run_ui(raw_input, |ui| build_ui(ui, workload, model, doc, scroll_y));
        state.handle_platform_output(window, output.platform_output);
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        self.driver.lap(Phase::Build);

        let gfx = self.gfx.as_mut().unwrap();

        let view = frame.texture.create_view(&Default::default());
        let mut encoder = gfx.device.create_command_encoder(&Default::default());
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                gfx.renderer
                    .update_texture(&gfx.device, &gfx.queue, *id, delta);
            }
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [gfx.config.width, gfx.config.height],
            pixels_per_point: output.pixels_per_point,
        };
        let mut commands = gfx.renderer.update_buffers(
            &gfx.device,
            &gfx.queue,
            &mut encoder,
            &primitives,
            &screen,
        );
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                timestamp_writes: gfx.timer.as_ref().map(GpuTimer::render_pass_writes),
                ..Default::default()
            });
            gfx.renderer
                .render(&mut pass.forget_lifetime(), &primitives, &screen);
        }
        if let Some(timer) = &gfx.timer {
            timer.resolve(&mut encoder);
        }
        commands.push(encoder.finish());
        gfx.queue.submit(commands);
        for id in &output.textures_delta.free {
            gfx.renderer.free_texture(id);
        }
        self.driver.lap(Phase::RasterCpu);

        window.pre_present_notify();
        gfx.queue.present(frame);
        self.driver.lap(Phase::Present);
        self.driver.presented();
        if let Some(timer) = &gfx.timer {
            self.driver
                .add(Phase::RasterGpu, timer.read_us(&gfx.device));
        }
        self.driver.end_frame();
        window.request_redraw();
    }
}

fn gpu_timing(args: &Args, supported: bool) -> String {
    match (args.gpu_timing, supported) {
        (false, _) => "disabled",
        (true, true) => "timestamp-query",
        (true, false) => "unsupported",
    }
    .into()
}

fn build_ui(
    ui: &mut egui::Ui,
    workload: Workload,
    model: &TableModel,
    doc: &[Block],
    scroll_y: f32,
) {
    egui::CentralPanel::default().show(ui, |ui| match workload {
        Workload::Table => {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    for row in &model.rows {
                        ui.horizontal(|ui| {
                            ui.add_sized([60.0, 20.0], egui::Label::new(row.id.to_string()));
                            let _ = ui.selectable_label(
                                model.selected == Some(row.id),
                                row.label.as_str(),
                            );
                            let _ = ui.button("x");
                        });
                    }
                });
        }
        Workload::Scroll => {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .vertical_scroll_offset(scroll_y)
                .show(ui, |ui| {
                    for block in doc {
                        let text = RichText::new(block.text.as_str()).size(block.kind.font_size());
                        ui.label(match block.kind {
                            BlockKind::Heading => text.strong(),
                            BlockKind::Paragraph => text,
                            BlockKind::Emphasis => text.italics(),
                            BlockKind::Code => text.monospace(),
                        });
                    }
                });
        }
    });
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            self.init(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let (Some(window), Some(state)) = (&self.window, &mut self.state) {
            let _ = state.on_window_event(window, &event);
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }
}

fn main() {
    let args = Args::parse();
    let driver = Driver::new("egui", &args);
    let mut app = App {
        args,
        driver,
        window: None,
        gfx: None,
        ctx: {
            let ctx = egui::Context::default();
            ctx.set_visuals(egui::Visuals::light());
            ctx
        },
        state: None,
        model: TableModel::default(),
        doc: bench_core::scroll::document(),
        scroll_y: 0.0,
    };
    EventLoop::new().unwrap().run_app(&mut app).unwrap();
}
