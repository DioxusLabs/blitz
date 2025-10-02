use crate::{
    CustomPaintSource,
    wgpu_context::{DeviceHandle, RenderSurface, WGPUContext},
};
use anyrender::{WindowHandle, WindowRenderer};
use debug_timer::debug_timer;
use peniko::Color;
use rustc_hash::FxHashMap;
use std::sync::{
    Arc,
    atomic::{self, AtomicU64},
};
use vello::{
    AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions, Scene as VelloScene,
};
use wgpu::{CommandEncoderDescriptor, Features, Limits, PresentMode, TextureViewDescriptor};

use crate::{DEFAULT_THREADS, VelloScenePainter};

static PAINT_SOURCE_ID: AtomicU64 = AtomicU64::new(0);

// Simple struct to hold the state of the renderer
struct ActiveRenderState {
    renderer: VelloRenderer,
    surface: RenderSurface<'static>,
}

#[allow(clippy::large_enum_variant)]
enum RenderState {
    Active(ActiveRenderState),
    Suspended,
}

impl RenderState {
    fn current_device_handle(&self) -> Option<&DeviceHandle> {
        let RenderState::Active(state) = self else {
            return None;
        };
        Some(&state.surface.device_handle)
    }
}

pub struct VelloWindowRenderer {
    // The fields MUST be in this order, so that the surface is dropped before the window
    // Window is cached even when suspended so that it can be reused when the app is resumed after being suspended
    render_state: RenderState,
    window_handle: Option<Arc<dyn WindowHandle>>,

    // Vello
    wgpu_context: WGPUContext,
    scene: Option<VelloScene>,

    custom_paint_sources: FxHashMap<u64, Box<dyn CustomPaintSource>>,
}
impl VelloWindowRenderer {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::with_features_and_limits(None, None)
    }

    pub fn with_features_and_limits(features: Option<Features>, limits: Option<Limits>) -> Self {
        let features =
            features.unwrap_or_default() | Features::CLEAR_TEXTURE | Features::PIPELINE_CACHE;
        Self {
            wgpu_context: WGPUContext::with_features_and_limits(Some(features), limits),
            render_state: RenderState::Suspended,
            window_handle: None,
            scene: Some(VelloScene::new()),
            custom_paint_sources: FxHashMap::default(),
        }
    }

    pub fn current_device_handle(&self) -> Option<&DeviceHandle> {
        self.render_state.current_device_handle()
    }

    pub fn register_custom_paint_source(&mut self, mut source: Box<dyn CustomPaintSource>) -> u64 {
        if let Some(device_handle) = self.render_state.current_device_handle() {
            let instance = &self.wgpu_context.instance;
            source.resume(instance, device_handle);
        }
        let id = PAINT_SOURCE_ID.fetch_add(1, atomic::Ordering::SeqCst);
        self.custom_paint_sources.insert(id, source);

        id
    }

    pub fn unregister_custom_paint_source(&mut self, id: u64) {
        if let Some(mut source) = self.custom_paint_sources.remove(&id) {
            source.suspend();
            drop(source);
        }
    }
}

impl WindowRenderer for VelloWindowRenderer {
    type ScenePainter<'a>
        = VelloScenePainter<'a>
    where
        Self: 'a;

    fn is_active(&self) -> bool {
        matches!(self.render_state, RenderState::Active(_))
    }

    fn resume(&mut self, window_handle: Arc<dyn WindowHandle>, width: u32, height: u32) {
        let surface = pollster::block_on(self.wgpu_context.create_surface(
            window_handle.clone(),
            width,
            height,
            PresentMode::AutoVsync,
        ))
        .expect("Error creating surface");

        self.window_handle = Some(window_handle);

        let options = RendererOptions {
            antialiasing_support: AaSupport::all(),
            use_cpu: false,
            num_init_threads: DEFAULT_THREADS,
            // TODO: add pipeline cache
            pipeline_cache: None,
        };

        let renderer = VelloRenderer::new(&surface.device_handle.device, options).unwrap();

        self.render_state = RenderState::Active(ActiveRenderState { renderer, surface });

        let device_handle = self.render_state.current_device_handle().unwrap();
        let instance = &self.wgpu_context.instance;
        for source in self.custom_paint_sources.values_mut() {
            source.resume(instance, device_handle)
        }
    }

    fn suspend(&mut self) {
        for source in self.custom_paint_sources.values_mut() {
            source.suspend()
        }
        self.render_state = RenderState::Suspended;
    }

    fn set_size(&mut self, width: u32, height: u32) {
        if let RenderState::Active(state) = &mut self.render_state {
            state.surface.resize(width, height);
        };
    }

    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };

        let surface = &state.surface;
        let device_handle = &surface.device_handle;

        debug_timer!(timer, feature = "log_frame_times");

        let render_params = RenderParams {
            base_color: Color::WHITE,
            width: state.surface.config.width,
            height: state.surface.config.height,
            antialiasing_method: vello::AaConfig::Msaa16,
        };

        // Regenerate the vello scene
        let mut scene = VelloScenePainter {
            inner: self.scene.take().unwrap(),
            renderer: &mut state.renderer,
            custom_paint_sources: &mut self.custom_paint_sources,
        };
        draw_fn(&mut scene);
        self.scene = Some(scene.finish());
        timer.record_time("cmd");

        state
            .renderer
            .render_to_texture(
                &device_handle.device,
                &device_handle.queue,
                self.scene.as_ref().unwrap(),
                &surface.target_view,
                &render_params,
            )
            .expect("failed to render to texture");
        timer.record_time("render");

        // TODO: verify that handling of SurfaceError::Outdated is no longer required
        //
        // let surface_texture = match state.surface.surface.get_current_texture() {
        //     Ok(surface) => surface,
        //     // When resizing too aggresively, the surface can get outdated (another resize) before being rendered into
        //     Err(SurfaceError::Outdated) => return,
        //     Err(_) => panic!("failed to get surface texture"),
        // };

        let surface_texture = state
            .surface
            .surface
            .get_current_texture()
            .expect("failed to get surface texture");

        // Perform the copy
        // (TODO: Does it improve throughput to acquire the surface after the previous texture render has happened?)
        let mut encoder = device_handle
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Surface Blit"),
            });

        state.surface.blitter.copy(
            &device_handle.device,
            &mut encoder,
            &surface.target_view,
            &surface_texture
                .texture
                .create_view(&TextureViewDescriptor::default()),
        );
        device_handle.queue.submit([encoder.finish()]);
        surface_texture.present();
        timer.record_time("present");

        let _ = device_handle.device.poll(wgpu::PollType::Wait);

        timer.record_time("wait");
        timer.print_times("Frame time: ");

        // static COUNTER: AtomicU64 = AtomicU64::new(0);
        // println!("FRAME {}", COUNTER.fetch_add(1, atomic::Ordering::Relaxed));

        // Empty the Vello scene (memory optimisation)
        self.scene.as_mut().unwrap().reset();
    }
}
