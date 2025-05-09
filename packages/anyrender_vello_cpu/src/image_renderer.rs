use crate::VelloCpuAnyrenderScene;
use anyrender::ImageRenderer;
use vello_cpu::{RenderContext, RenderMode};

pub struct VelloCpuImageRenderer {
    scene: VelloCpuAnyrenderScene,
}

impl ImageRenderer for VelloCpuImageRenderer {
    type Scene = VelloCpuAnyrenderScene;

    fn new(width: u32, height: u32) -> Self {
        Self {
            scene: VelloCpuAnyrenderScene(RenderContext::new(width as u16, height as u16)),
        }
    }

    fn render<F: FnOnce(&mut Self::Scene)>(&mut self, draw_fn: F, buffer: &mut Vec<u8>) {
        let width = self.scene.0.width();
        let height = self.scene.0.height();
        draw_fn(&mut self.scene);
        buffer.resize(width as usize * height as usize * 4, 0);
        self.scene
            .0
            .render_to_buffer(&mut *buffer, width, height, RenderMode::OptimizeSpeed);
    }
}
