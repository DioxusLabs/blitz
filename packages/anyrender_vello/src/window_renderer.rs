use crate::{
    CustomPaintSource,
    wgpu_context::{RenderSurface, WGPUContext},
};
use anyrender::{PaintScene as _, WindowHandle, WindowRenderer};
use peniko::Color;
use rustc_hash::FxHashMap;
use std::sync::{
    Arc,
    atomic::{self, AtomicU64},
};
use vello::{
    AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions, Scene as VelloScene,
};
use wgpu::{
    CommandEncoderDescriptor, Device, Features, Limits, PresentMode, Queue, TextureViewDescriptor,
};

use crate::{DEFAULT_THREADS, VelloAnyrenderScene};

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
    fn current_device_and_queue(&self) -> Option<(&Device, &Queue)> {
        let RenderState::Active(state) = self else {
            return None;
        };

        let device_handle = &state.surface.device_handle;
        let device = &device_handle.device;
        let queue = &device_handle.queue;

        Some((device, queue))
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

    pub fn current_device_and_queue(&self) -> Option<(&Device, &Queue)> {
        self.render_state.current_device_and_queue()
    }

    pub fn register_custom_paint_source(&mut self, mut source: Box<dyn CustomPaintSource>) -> u64 {
        if let Some((device, queue)) = self.render_state.current_device_and_queue() {
            source.resume(device, queue);
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
    type Scene<'a>
        = VelloAnyrenderScene<'a>
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

        let (device, queue) = self.render_state.current_device_and_queue().unwrap();
        for source in self.custom_paint_sources.values_mut() {
            source.resume(device, queue)
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

    fn render<F: FnOnce(&mut Self::Scene<'_>)>(&mut self, draw_fn: F) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };

        let surface = &state.surface;
        let device_handle = &surface.device_handle;

        let render_params = RenderParams {
            base_color: Color::WHITE,
            width: state.surface.config.width,
            height: state.surface.config.height,
            antialiasing_method: vello::AaConfig::Msaa16,
        };

        // Regenerate the vello scene
        let mut scene = VelloAnyrenderScene {
            inner: self.scene.take().unwrap(),
            renderer: &mut state.renderer,
            custom_paint_sources: &mut self.custom_paint_sources,
        };
        draw_fn(&mut scene);
        self.scene = Some(scene.finish());

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

        device_handle.device.poll(wgpu::Maintain::Wait);

        // static COUNTER: AtomicU64 = AtomicU64::new(0);
        // println!("FRAME {}", COUNTER.fetch_add(1, atomic::Ordering::Relaxed));

        // Empty the Vello scene (memory optimisation)
        self.scene.as_mut().unwrap().reset();
    }
}
