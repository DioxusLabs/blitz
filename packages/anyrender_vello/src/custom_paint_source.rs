use peniko::ImageData;
use vello::Renderer as VelloRenderer;
use wgpu::{Instance, Texture};
use wgpu_context::DeviceHandle;

pub trait CustomPaintSource: 'static {
    fn resume(&mut self, instance: &Instance, device_handle: &DeviceHandle);
    fn suspend(&mut self);
    fn render(
        &mut self,
        ctx: CustomPaintCtx<'_>,
        width: u32,
        height: u32,
        scale: f64,
    ) -> Option<TextureHandle>;
}

pub struct CustomPaintCtx<'r> {
    pub(crate) renderer: &'r mut VelloRenderer,
}

#[derive(Clone, PartialEq)]
pub struct TextureHandle(pub ImageData);

impl CustomPaintCtx<'_> {
    pub(crate) fn new<'a>(renderer: &'a mut VelloRenderer) -> CustomPaintCtx<'a> {
        CustomPaintCtx { renderer }
    }

    pub fn register_texture(&mut self, texture: Texture) -> TextureHandle {
        TextureHandle(self.renderer.register_texture(texture))
    }

    pub fn unregister_texture(&mut self, handle: TextureHandle) {
        self.renderer.unregister_texture(handle.0);
    }
}
