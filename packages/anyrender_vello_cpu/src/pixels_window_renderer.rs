use crate::VelloCpuScenePainter;
use crate::vello_cpu::{RenderContext, RenderMode};
use anyrender::{WindowHandle, WindowRenderer};
use pixels::{Pixels, SurfaceTexture, wgpu::Color};
use std::{sync::Arc, time::Instant};

// Simple struct to hold the state of the renderer
pub struct ActiveRenderState {
    // surface: SurfaceTexture<Arc<dyn WindowHandle>>,
    pixels: Pixels<'static>,
}

#[allow(clippy::large_enum_variant)]
pub enum RenderState {
    Active(ActiveRenderState),
    Suspended,
}

pub struct VelloCpuWindowRenderer {
    // The fields MUST be in this order, so that the surface is dropped before the window
    // Window is cached even when suspended so that it can be reused when the app is resumed after being suspended
    render_state: RenderState,
    window_handle: Option<Arc<dyn WindowHandle>>,
    render_context: VelloCpuScenePainter,
}

impl VelloCpuWindowRenderer {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            render_state: RenderState::Suspended,
            window_handle: None,
            render_context: VelloCpuScenePainter(RenderContext::new(0, 0)),
        }
    }
}

impl WindowRenderer for VelloCpuWindowRenderer {
    type ScenePainter<'a> = VelloCpuScenePainter;

    fn is_active(&self) -> bool {
        matches!(self.render_state, RenderState::Active(_))
    }

    fn resume(&mut self, window_handle: Arc<dyn WindowHandle>, width: u32, height: u32) {
        let surface = SurfaceTexture::new(width, height, window_handle.clone());
        let mut pixels = Pixels::new(width, height, surface).unwrap();
        pixels.enable_vsync(true);
        pixels.clear_color(Color {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        });
        self.render_state = RenderState::Active(ActiveRenderState { pixels });
        self.window_handle = Some(window_handle);

        self.set_size(width, height);
    }

    fn suspend(&mut self) {
        self.render_state = RenderState::Suspended;
    }

    fn set_size(&mut self, physical_width: u32, physical_height: u32) {
        if let RenderState::Active(state) = &mut self.render_state {
            state
                .pixels
                .resize_buffer(physical_width, physical_height)
                .unwrap();
            state
                .pixels
                .resize_surface(physical_width, physical_height)
                .unwrap();
            self.render_context = VelloCpuScenePainter(RenderContext::new(
                physical_width as u16,
                physical_height as u16,
            ));
        };
    }

    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };

        let start = Instant::now();

        // Paint
        let width = self.render_context.0.width();
        let height = self.render_context.0.height();
        // let mut pixmap = Pixmap::new(width, height);
        draw_fn(&mut self.render_context);

        let command_time = start.elapsed().as_millis();
        let command_ms = command_time;

        self.render_context.0.render_to_buffer(
            state.pixels.frame_mut(),
            width,
            height,
            RenderMode::OptimizeSpeed,
        );

        let render_time = start.elapsed().as_millis();
        let render_ms = render_time - command_time;

        // No "swizel" step with pixels renderer

        let swizel_time = start.elapsed().as_millis();
        let swizel_ms = swizel_time - render_time;

        state.pixels.render().unwrap();
        // surface_buffer.present().unwrap();

        let present_time = start.elapsed().as_millis();
        let present_ms = present_time - render_time;

        let overall_ms = present_time;

        // Empty the Vello render context (memory optimisation)
        self.render_context.0.reset();

        println!(
            "Frame time: {overall_ms}ms (cmd: {command_ms}ms, render: {render_ms}ms, swizel: {swizel_ms}ms, present: {present_ms}ms)"
        );
    }
}
