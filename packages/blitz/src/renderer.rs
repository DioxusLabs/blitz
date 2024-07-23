mod multicolor_rounded_rect;
mod render;

use crate::devtools::Devtools;
use blitz_dom::{Document, Viewport};
use std::num::NonZeroUsize;
use std::sync::Arc;
use vello::{
    peniko::Color, util::RenderContext, util::RenderSurface, AaSupport, RenderParams,
    Renderer as VelloRenderer, RendererOptions, Scene,
};
use wgpu::{PresentMode, SurfaceError, WasmNotSend};

// Simple struct to hold the state of the renderer
pub struct ActiveRenderState<'s> {
    renderer: VelloRenderer,
    surface: RenderSurface<'s>,
}

pub enum RenderState<'s> {
    Active(ActiveRenderState<'s>),
    Suspended,
}

pub struct Renderer<'s, W>
where
    W: raw_window_handle::HasWindowHandle
        + raw_window_handle::HasDisplayHandle
        + Sync
        + WasmNotSend
        + 's,
{
    // The fields MUST be in this order, so that the surface is dropped before the window
    // Window is cached even when suspended so that it can be reused when the app is resumed after being suspended
    pub render_state: RenderState<'s>,
    pub window: Arc<W>,

    // Vello
    pub(crate) render_context: RenderContext,
    pub(crate) scene: Scene,
}

impl<'a, W> Renderer<'a, W>
where
    W: raw_window_handle::HasWindowHandle
        + raw_window_handle::HasDisplayHandle
        + Sync
        + WasmNotSend
        + 'a,
{
    pub fn new(window: Arc<W>) -> Self {
        // 1. Set up renderer-specific stuff
        // We build an independent viewport which can be dynamically set later
        // The intention here is to split the rendering pipeline away from tao/windowing for rendering to images

        // 2. Set up Vello specific stuff
        let render_context = RenderContext::new().unwrap();

        Self {
            render_context,
            render_state: RenderState::Suspended,
            window,
            scene: Scene::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.render_state, RenderState::Active(_))
    }

    pub async fn resume(&mut self, viewport: &Viewport) {
        let surface = self
            .render_context
            .create_surface(
                self.window.clone(),
                viewport.window_size.0,
                viewport.window_size.1,
                PresentMode::AutoVsync,
            )
            .await
            .expect("Error creating surface");

        const DEFAULT_THREADS: Option<NonZeroUsize> = {
            #[cfg(target_os = "macos")]
            {
                NonZeroUsize::new(1)
            }
            #[cfg(not(target_os = "macos"))]
            {
                None
            }
        };

        let options = RendererOptions {
            surface_format: Some(surface.config.format),
            antialiasing_support: AaSupport::all(),
            use_cpu: false,
            num_init_threads: DEFAULT_THREADS,
        };

        let renderer =
            VelloRenderer::new(&self.render_context.devices[surface.dev_id].device, options)
                .unwrap();

        self.render_state = RenderState::Active(ActiveRenderState { renderer, surface });
    }

    pub fn suspend(&mut self) {
        self.render_state = RenderState::Suspended;
    }

    // Adjust the viewport
    pub fn set_size(&mut self, physical_width: u32, physical_height: u32) {
        if let RenderState::Active(state) = &mut self.render_state {
            self.render_context
                .resize_surface(&mut state.surface, physical_width, physical_height);
        };
    }

    pub fn render(&mut self, doc: &Document, scale: f64, devtools: Devtools) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };
        let surface_texture = match state.surface.surface.get_current_texture() {
            Ok(surface) => surface,
            // When resizing too aggresively, the surface can get outdated (another resize) before being rendered into
            Err(SurfaceError::Outdated) => return,
            Err(_) => panic!("failed to get surface texture"),
        };

        let device = &self.render_context.devices[state.surface.dev_id];

        let render_params = RenderParams {
            base_color: Color::WHITE,
            width: state.surface.config.width,
            height: state.surface.config.height,
            antialiasing_method: vello::AaConfig::Msaa16,
        };

        // Regenerate the vello scene
        render::generate_vello_scene(&mut self.scene, doc, scale, devtools);

        state
            .renderer
            .render_to_surface(
                &device.device,
                &device.queue,
                &self.scene,
                &surface_texture,
                &render_params,
            )
            .expect("failed to render to surface");

        surface_texture.present();
        device.device.poll(wgpu::Maintain::Wait);

        // Empty the Vello scene (memory optimisation)
        self.scene.reset();
    }
}
