use std::{num::NonZero, sync::Arc};

use anyrender_vello_cpu::VelloCpuAnyrenderScene;
use blitz_dom::BaseDocument;
use blitz_paint::paint_scene;
use blitz_traits::{BlitzWindowHandle, Devtools, DocumentRenderer, Viewport};
use softbuffer::{Context, Surface};
use vello_cpu::{Pixmap, RenderContext};

pub async fn render_to_buffer(dom: &BaseDocument, viewport: Viewport) -> Vec<u8> {
    let (width, height) = viewport.window_size;

    let scene = RenderContext::new(width as u16, height as u16);
    let mut anyrender_scene = VelloCpuAnyrenderScene(scene);
    paint_scene(
        &mut anyrender_scene,
        dom,
        viewport.scale_f64(),
        width,
        height,
        Devtools::default(),
    );

    let mut pixmap = Pixmap::new(width as u16, height as u16);
    anyrender_scene.0.render_to_pixmap(&mut pixmap);

    pixmap.buf
}

// Simple struct to hold the state of the renderer
pub struct ActiveRenderState {
    _context: Context<Arc<dyn BlitzWindowHandle>>,
    surface: Surface<Arc<dyn BlitzWindowHandle>, Arc<dyn BlitzWindowHandle>>,
}

#[allow(clippy::large_enum_variant)]
pub enum RenderState {
    Active(ActiveRenderState),
    Suspended,
}

pub struct BlitzVelloCpuRenderer {
    // The fields MUST be in this order, so that the surface is dropped before the window
    // Window is cached even when suspended so that it can be reused when the app is resumed after being suspended
    render_state: RenderState,
    window_handle: Arc<dyn BlitzWindowHandle>,
    render_context: VelloCpuAnyrenderScene,
}

impl DocumentRenderer for BlitzVelloCpuRenderer {
    type Doc = BaseDocument;

    fn new(window: Arc<dyn BlitzWindowHandle>) -> Self {
        Self {
            render_state: RenderState::Suspended,
            window_handle: window,
            render_context: VelloCpuAnyrenderScene(RenderContext::new(0, 0)),
        }
    }

    fn is_active(&self) -> bool {
        matches!(self.render_state, RenderState::Active(_))
    }

    fn resume(&mut self, viewport: &Viewport) {
        let context = Context::new(self.window_handle.clone()).unwrap();
        let surface = Surface::new(&context, self.window_handle.clone()).unwrap();
        self.render_state = RenderState::Active(ActiveRenderState {
            _context: context,
            surface,
        });

        let (width, height) = viewport.window_size;
        self.set_size(width, height);
        self.render_context = VelloCpuAnyrenderScene(RenderContext::new(0, 0));
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
            self.render_context = VelloCpuAnyrenderScene(RenderContext::new(
                physical_width as u16,
                physical_height as u16,
            ));
        };
    }

    fn render(
        &mut self,
        doc: &BaseDocument,
        scale: f64,
        width: u32,
        height: u32,
        devtools: Devtools,
    ) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };
        let Ok(mut surface_buffer) = state.surface.buffer_mut() else {
            return;
        };

        // Paint
        let mut pixmap = Pixmap::new(width as u16, height as u16);
        paint_scene(
            &mut self.render_context,
            doc,
            scale,
            width,
            height,
            devtools,
        );
        self.render_context.0.render_to_pixmap(&mut pixmap);

        let out = surface_buffer.as_mut();
        assert_eq!(pixmap.buf.len(), out.len() * 4);
        for (src, dest) in pixmap.buf.chunks_exact_mut(4).zip(out.iter_mut()) {
            let [r, g, b, a] = *src else {
                panic!();
            };
            if a == 0 {
                *dest = u32::MAX;
            } else {
                *dest = (r as u32) << 16 | (g as u32) << 8 | b as u32;
            }
        }

        surface_buffer.present().unwrap();

        // Empty the Vello render context (memory optimisation)
        self.render_context.0.reset();
    }
}
