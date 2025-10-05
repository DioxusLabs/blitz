use anyrender::ImageRenderer;
use rustc_hash::FxHashMap;
use vello::{Renderer as VelloRenderer, RendererOptions, Scene as VelloScene};
use wgpu::TextureUsages;
use wgpu_context::{BufferRenderer, BufferRendererConfig, WGPUContext};

use crate::{DEFAULT_THREADS, VelloScenePainter};

pub struct VelloImageRenderer {
    buffer_renderer: BufferRenderer,
    vello_renderer: VelloRenderer,

    // scene is always Some except temporarily during when it is moved out
    // to keep the borrow-checker happy.
    scene: Option<VelloScene>,
}

impl ImageRenderer for VelloImageRenderer {
    type ScenePainter<'a>
        = VelloScenePainter<'a>
    where
        Self: 'a;

    fn new(width: u32, height: u32) -> Self {
        // Create WGPUContext
        let mut context = WGPUContext::new();

        // Create wgpu_context::BufferRenderer
        let buffer_renderer =
            pollster::block_on(context.create_buffer_renderer(BufferRendererConfig {
                width,
                height,
                usage: TextureUsages::STORAGE_BINDING,
            }))
            .expect("No compatible device found");

        // Create vello::Renderer
        let vello_renderer = VelloRenderer::new(
            buffer_renderer.device(),
            RendererOptions {
                use_cpu: false,
                num_init_threads: DEFAULT_THREADS,
                antialiasing_support: vello::AaSupport::area_only(),
                pipeline_cache: None,
            },
        )
        .expect("Got non-Send/Sync error from creating renderer");

        Self {
            buffer_renderer,
            vello_renderer,
            scene: Some(VelloScene::new()),
        }
    }

    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(
        &mut self,
        draw_fn: F,
        cpu_buffer: &mut Vec<u8>,
    ) {
        let mut scene = VelloScenePainter {
            inner: self.scene.take().unwrap(),
            renderer: &mut self.vello_renderer,
            custom_paint_sources: &mut FxHashMap::default(),
        };
        draw_fn(&mut scene);
        self.scene = Some(scene.finish());

        let size = self.buffer_renderer.size();
        self.vello_renderer
            .render_to_texture(
                self.buffer_renderer.device(),
                self.buffer_renderer.queue(),
                self.scene.as_ref().unwrap(),
                &self.buffer_renderer.target_texture_view(),
                &vello::RenderParams {
                    base_color: vello::peniko::Color::TRANSPARENT,
                    width: size.width,
                    height: size.height,
                    antialiasing_method: vello::AaConfig::Area,
                },
            )
            .expect("Got non-Send/Sync error from rendering");

        self.buffer_renderer.copy_texture_to_buffer(cpu_buffer);

        // Empty the Vello scene (memory optimisation)
        self.scene.as_mut().unwrap().reset();
    }
}
