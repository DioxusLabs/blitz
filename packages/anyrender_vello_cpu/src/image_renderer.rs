use crate::VelloCpuScenePainter;
use anyrender::ImageRenderer;
use vello_cpu::{RenderContext, RenderMode};

pub struct VelloCpuImageRenderer {
    scene: VelloCpuScenePainter,
}

impl ImageRenderer for VelloCpuImageRenderer {
    type ScenePainter<'a> = VelloCpuScenePainter;

    fn new(width: u32, height: u32) -> Self {
        Self {
            scene: VelloCpuScenePainter(RenderContext::new(width as u16, height as u16)),
        }
    }

    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F, buffer: &mut Vec<u8>) {
        let width = self.scene.0.width();
        let height = self.scene.0.height();
        draw_fn(&mut self.scene);
        buffer.resize(width as usize * height as usize * 4, 0);
        self.scene
            .0
            .render_to_buffer(&mut *buffer, width, height, RenderMode::OptimizeSpeed);
    }
}
