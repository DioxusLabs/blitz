mod multicolor_rounded_rect;
mod render;

use crate::renderer::render::generate_vello_scene;
use blitz_dom::{Document, Viewport};
use blitz_traits::Devtools;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::num::NonZeroUsize;
use std::sync::Arc;
use vello::{
    peniko::Color,
    util::{block_on_wgpu, RenderContext, RenderSurface},
    AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions, Scene,
};
use wgpu::{
    BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, ImageCopyBuffer,
    PresentMode, SurfaceError, TextureDescriptor, TextureFormat, TextureUsages, WasmNotSendSync,
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
    W: HasWindowHandle + HasDisplayHandle + WasmNotSendSync + 's,
{
    // The fields MUST be in this order, so that the surface is dropped before the window
    // Window is cached even when suspended so that it can be reused when the app is resumed after being suspended
    pub render_state: RenderState<'s>,
    pub window_handle: Arc<W>,

    // Vello
    pub(crate) render_context: RenderContext,
    pub(crate) scene: Scene,
}

impl<'a, W> Renderer<'a, W>
where
    W: HasWindowHandle + HasDisplayHandle + WasmNotSendSync + 'a,
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
            window_handle: window,
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
                self.window_handle.clone(),
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

    pub fn render(
        &mut self,
        doc: &Document,
        scale: f64,
        width: u32,
        height: u32,
        devtools: Devtools,
    ) {
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
        render::generate_vello_scene(&mut self.scene, doc, scale, width, height, devtools);

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

pub struct VelloImageRenderer {
    size: Extent3d,
    scale: f64,
    // render_context: vello::util::RenderContext,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello::Renderer,
    scene: vello::Scene,
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    gpu_buffer: wgpu::Buffer,
}

impl VelloImageRenderer {
    pub async fn new(width: u32, height: u32, scale: f64) -> Self {
        let size = Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        // Create render context
        let mut context = RenderContext::new();

        // Setup device
        let device_id = context
            .device(None)
            .await
            .expect("No compatible device found");
        let device_handle = context.devices.remove(device_id);
        let device = device_handle.device;
        let queue = device_handle.queue;

        // Create renderer
        let renderer = vello::Renderer::new(
            &device,
            RendererOptions {
                surface_format: None,
                use_cpu: false,
                num_init_threads: DEFAULT_THREADS,
                antialiasing_support: vello::AaSupport::area_only(),
            },
        )
        .expect("Got non-Send/Sync error from creating renderer");

        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Target texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let padded_byte_width = (width * 4).next_multiple_of(256);
        let buffer_size = padded_byte_width as u64 * height as u64;
        let gpu_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("val"),
            size: buffer_size,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            size,
            scale,
            device,
            queue,
            renderer,
            texture,
            texture_view,
            gpu_buffer,
            scene: Scene::new(),
        }
    }

    pub fn render_document(&mut self, doc: &Document, cpu_buffer: &mut Vec<u8>) {
        generate_vello_scene(
            &mut self.scene,
            doc,
            self.scale,
            self.size.width,
            self.size.height,
            Devtools::default(),
        );

        self.render_internal_scene(cpu_buffer);
    }

    fn render_internal_scene(&mut self, cpu_buffer: &mut Vec<u8>) {
        let render_params = vello::RenderParams {
            base_color: vello::peniko::Color::WHITE,
            width: self.size.width,
            height: self.size.height,
            antialiasing_method: vello::AaConfig::Area,
        };

        self.renderer
            .render_to_texture(
                &self.device,
                &self.queue,
                &self.scene,
                &self.texture_view,
                &render_params,
            )
            .expect("Got non-Send/Sync error from rendering");

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Copy out buffer"),
            });
        let padded_byte_width = (self.size.width * 4).next_multiple_of(256);
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            ImageCopyBuffer {
                buffer: &self.gpu_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_byte_width),
                    rows_per_image: None,
                },
            },
            self.size,
        );

        self.queue.submit([encoder.finish()]);
        let buf_slice = self.gpu_buffer.slice(..);

        let (sender, receiver) = futures_intrusive::channel::shared::oneshot_channel();
        buf_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
        if let Some(recv_result) = block_on_wgpu(&self.device, receiver.receive()) {
            recv_result.unwrap();
        } else {
            panic!("channel was closed");
        }

        let data = buf_slice.get_mapped_range();

        cpu_buffer.clear();
        cpu_buffer.reserve((self.size.width * self.size.height * 4) as usize);

        // Pad result
        for row in 0..self.size.height {
            let start = (row * padded_byte_width).try_into().unwrap();
            cpu_buffer.extend(&data[start..start + (self.size.width * 4) as usize]);
        }

        // Unmap buffer
        drop(data);
        self.gpu_buffer.unmap();

        // Empty the Vello scene (memory optimisation)
        self.scene.reset();
    }
}

pub async fn render_to_buffer(dom: &Document, viewport: Viewport) -> Vec<u8> {
    let (width, height) = viewport.window_size;

    let mut buf = Vec::with_capacity((width * height * 4) as usize);
    let mut renderer = VelloImageRenderer::new(width, height, viewport.scale_f64()).await;
    renderer.render_document(dom, &mut buf);

    buf
}
