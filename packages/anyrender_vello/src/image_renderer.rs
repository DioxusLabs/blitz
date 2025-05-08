use anyrender::{ImageRenderer, Scene as _};
use vello::{
    RendererOptions, Scene,
    util::{RenderContext, block_on_wgpu},
};
use wgpu::{
    BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, TexelCopyBufferInfo,
    TexelCopyBufferLayout, TextureDescriptor, TextureFormat, TextureUsages,
};

use crate::{DEFAULT_THREADS, VelloAnyrenderScene};

pub struct VelloImageRenderer {
    size: Extent3d,
    // render_context: vello::util::RenderContext,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello::Renderer,
    scene: VelloAnyrenderScene,
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    gpu_buffer: wgpu::Buffer,
}

impl ImageRenderer for VelloImageRenderer {
    type Scene = VelloAnyrenderScene;

    fn new(width: u32, height: u32) -> Self {
        let size = Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        // Create render context
        let mut context = RenderContext::new();

        // Setup device
        let device_id =
            pollster::block_on(context.device(None)).expect("No compatible device found");
        let device_handle = context.devices.remove(device_id);
        let device = device_handle.device;
        let queue = device_handle.queue;

        // Create renderer
        let renderer = vello::Renderer::new(
            &device,
            RendererOptions {
                use_cpu: false,
                num_init_threads: DEFAULT_THREADS,
                antialiasing_support: vello::AaSupport::area_only(),
                pipeline_cache: None,
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
            device,
            queue,
            renderer,
            texture,
            texture_view,
            gpu_buffer,
            scene: VelloAnyrenderScene(Scene::new()),
        }
    }

    fn render<F: FnOnce(&mut Self::Scene)>(&mut self, draw_fn: F, cpu_buffer: &mut Vec<u8>) {
        draw_fn(&mut self.scene);
        self.render_internal_scene(cpu_buffer);
    }
}

impl VelloImageRenderer {
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
                &self.scene.0,
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
            TexelCopyBufferInfo {
                buffer: &self.gpu_buffer,
                layout: TexelCopyBufferLayout {
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
