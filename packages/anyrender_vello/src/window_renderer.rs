use anyrender::{Scene as _, WindowHandle, WindowRenderer};
use peniko::Color;
use std::sync::Arc;
use vello::{
    AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions, Scene,
    util::{RenderContext, RenderSurface},
};
use wgpu::{CommandEncoderDescriptor, PresentMode, TextureViewDescriptor};

use crate::{DEFAULT_THREADS, VelloAnyrenderScene};

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

pub struct VelloWindowRenderer {
    // The fields MUST be in this order, so that the surface is dropped before the window
    // Window is cached even when suspended so that it can be reused when the app is resumed after being suspended
    render_state: RenderState,
    window_handle: Option<Arc<dyn WindowHandle>>,

    // Vello
    render_context: RenderContext,
    scene: VelloAnyrenderScene,
}

impl VelloWindowRenderer {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        // 2. Set up Vello specific stuff
        let render_context = RenderContext::new();

        Self {
            render_context,
            render_state: RenderState::Suspended,
            window_handle: None,
            scene: VelloAnyrenderScene(Scene::new()),
        }
    }
}

impl WindowRenderer for VelloWindowRenderer {
    type Scene = VelloAnyrenderScene;

    fn is_active(&self) -> bool {
        matches!(self.render_state, RenderState::Active(_))
    }

    fn resume(&mut self, window_handle: Arc<dyn WindowHandle>, width: u32, height: u32) {
        let surface = pollster::block_on(self.render_context.create_surface(
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

        let renderer =
            VelloRenderer::new(&self.render_context.devices[surface.dev_id].device, options)
                .unwrap();

        self.render_state = RenderState::Active(ActiveRenderState { renderer, surface });
    }

    fn suspend(&mut self) {
        self.render_state = RenderState::Suspended;
    }

    fn set_size(&mut self, width: u32, height: u32) {
        if let RenderState::Active(state) = &mut self.render_state {
            self.render_context
                .resize_surface(&mut state.surface, width, height);
        };
    }

    fn render<F: FnOnce(&mut Self::Scene)>(&mut self, draw_fn: F) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };

        let device = &self.render_context.devices[state.surface.dev_id];
        let surface = &state.surface;

        let render_params = RenderParams {
            base_color: Color::WHITE,
            width: state.surface.config.width,
            height: state.surface.config.height,
            antialiasing_method: vello::AaConfig::Msaa16,
        };

        // Regenerate the vello scene
        draw_fn(&mut self.scene);

        state
            .renderer
            .render_to_texture(
                &device.device,
                &device.queue,
                &self.scene.0,
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
        let mut encoder = device
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Surface Blit"),
            });

        state.surface.blitter.copy(
            &device.device,
            &mut encoder,
            &surface.target_view,
            &surface_texture
                .texture
                .create_view(&TextureViewDescriptor::default()),
        );
        device.queue.submit([encoder.finish()]);
        surface_texture.present();

        device.device.poll(wgpu::Maintain::Wait);

        // Empty the Vello scene (memory optimisation)
        self.scene.reset();
    }
}
