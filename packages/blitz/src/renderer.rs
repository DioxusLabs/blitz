mod multicolor_rounded_rect;
mod render;

use crate::{devtools::Devtools, renderer::render::generate_vello_scene};
use blitz_dom::{Document, Viewport};
use std::num::NonZeroUsize;
use std::sync::Arc;
use vello::{
    block_on_wgpu,
    peniko::Color,
    util::{RenderContext, RenderSurface},
    AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions, Scene,
};
use wgpu::{
    BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, ImageCopyBuffer,
    PresentMode, SurfaceError, TextureDescriptor, TextureFormat, TextureUsages, WasmNotSend,
};

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
        let render_context = RenderContext::new();

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
            debug: vello::DebugLayers::none(),
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

pub async fn render_to_buffer(dom: &Document, viewport: Viewport) -> Vec<u8> {
    // Create render context
    let mut context = RenderContext::new();

    // Setup device
    let device_id = context
        .device(None)
        .await
        .expect("No compatible device found");
    let device_handle = &mut context.devices[device_id];
    let device = &device_handle.device;
    let queue = &device_handle.queue;

    // Create renderer
    let mut renderer = vello::Renderer::new(
        device,
        RendererOptions {
            surface_format: None,
            use_cpu: false,
            num_init_threads: DEFAULT_THREADS,
            antialiasing_support: vello::AaSupport::area_only(),
        },
    )
    .expect("Got non-Send/Sync error from creating renderer");

    let mut scene = Scene::new();
    generate_vello_scene(&mut scene, dom, viewport.scale_f64(), Devtools::default());

    let (width, height) = viewport.window_size;

    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&TextureDescriptor {
        label: Some("Target texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let render_params = vello::RenderParams {
        base_color: vello::peniko::Color::WHITE,
        width,
        height,
        antialiasing_method: vello::AaConfig::Area,
        debug: vello::DebugLayers::none(),
    };
    renderer
        .render_to_texture(device, queue, &scene, &view, &render_params)
        .expect("Got non-Send/Sync error from rendering");
    let padded_byte_width = (width * 4).next_multiple_of(256);
    let buffer_size = padded_byte_width as u64 * height as u64;
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("val"),
        size: buffer_size,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("Copy out buffer"),
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded_byte_width),
                rows_per_image: None,
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let buf_slice = buffer.slice(..);

    let (sender, receiver) = futures_intrusive::channel::shared::oneshot_channel();
    buf_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
    if let Some(recv_result) = block_on_wgpu(device, receiver.receive()) {
        recv_result.unwrap();
    } else {
        panic!("channel was closed");
    }

    let data = buf_slice.get_mapped_range();
    let mut result = Vec::<u8>::with_capacity((width * height * 4).try_into().unwrap());

    // Pad result
    for row in 0..height {
        let start = (row * padded_byte_width).try_into().unwrap();
        result.extend(&data[start..start + (width * 4) as usize]);
    }

    result
}
