use crate::VelloCpuScenePainter;
use anyrender::{WindowHandle, WindowRenderer};
use debug_timer::debug_timer;
use peniko::color::PremulRgba8;
use softbuffer::{Context, Surface};
use std::{num::NonZero, sync::Arc};
use vello_cpu::{Pixmap, RenderContext, RenderMode};

// Simple struct to hold the state of the renderer
pub struct ActiveRenderState {
    _context: Context<Arc<dyn WindowHandle>>,
    surface: Surface<Arc<dyn WindowHandle>, Arc<dyn WindowHandle>>,
}

#[allow(clippy::large_enum_variant)]
pub enum RenderState {
    Active(ActiveRenderState),
    Suspended,
}

pub struct VelloCpuSoftbufferWindowRenderer {
    // The fields MUST be in this order, so that the surface is dropped before the window
    // Window is cached even when suspended so that it can be reused when the app is resumed after being suspended
    render_state: RenderState,
    window_handle: Option<Arc<dyn WindowHandle>>,
    render_context: VelloCpuScenePainter,
}

impl VelloCpuSoftbufferWindowRenderer {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            render_state: RenderState::Suspended,
            window_handle: None,
            render_context: VelloCpuScenePainter(RenderContext::new(0, 0)),
        }
    }
}

impl WindowRenderer for VelloCpuSoftbufferWindowRenderer {
    type ScenePainter<'a> = VelloCpuScenePainter;

    fn is_active(&self) -> bool {
        matches!(self.render_state, RenderState::Active(_))
    }

    fn resume(&mut self, window_handle: Arc<dyn WindowHandle>, width: u32, height: u32) {
        let context = Context::new(window_handle.clone()).unwrap();
        let surface = Surface::new(&context, window_handle.clone()).unwrap();
        self.render_state = RenderState::Active(ActiveRenderState {
            _context: context,
            surface,
        });
        self.window_handle = Some(window_handle);

        self.set_size(width, height);
    }

    fn suspend(&mut self) {
        self.render_state = RenderState::Suspended;
    }

    fn set_size(&mut self, physical_width: u32, physical_height: u32) {
        if let RenderState::Active(state) = &mut self.render_state {
            state
                .surface
                .resize(
                    NonZero::new(physical_width.max(1)).unwrap(),
                    NonZero::new(physical_height.max(1)).unwrap(),
                )
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

        debug_timer!(timer, feature = "log_frame_times");

        let Ok(mut surface_buffer) = state.surface.buffer_mut() else {
            return;
        };
        timer.record_time("buffer_mut");

        // Paint
        let width = self.render_context.0.width();
        let height = self.render_context.0.height();
        let mut pixmap = Pixmap::new(width, height);
        draw_fn(&mut self.render_context);
        timer.record_time("cmd");

        self.render_context
            .0
            .render_to_pixmap(&mut pixmap, RenderMode::OptimizeSpeed);
        timer.record_time("render");

        let out = surface_buffer.as_mut();
        assert_eq!(pixmap.data().len(), out.len());
        for (src, dest) in pixmap.data().iter().zip(out.iter_mut()) {
            let PremulRgba8 { r, g, b, a } = *src;
            if a == 0 {
                *dest = u32::MAX;
            } else {
                *dest = (r as u32) << 16 | (g as u32) << 8 | b as u32;
            }
        }
        timer.record_time("swizel");

        surface_buffer.present().unwrap();
        timer.record_time("present");
        timer.print_times("Frame time: ");

        // Empty the Vello render context (memory optimisation)
        self.render_context.0.reset();
    }
}
