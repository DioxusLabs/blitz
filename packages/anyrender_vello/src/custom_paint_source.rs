use vello::Renderer as VelloRenderer;
use wgpu::{Device, Queue, Texture};

pub trait CustomPaintSource: 'static {
    fn resume(&mut self, device: &Device, queue: &Queue);
    fn suspend(&mut self);
    fn set_size(&mut self, width: u32, height: u32);
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

#[derive(Copy, Clone, PartialEq, Hash)]
pub struct TextureHandle(vello::TextureHandle);

impl From<vello::TextureHandle> for TextureHandle {
    fn from(value: vello::TextureHandle) -> Self {
        TextureHandle(value)
    }
}

impl From<TextureHandle> for vello::TextureHandle {
    fn from(value: TextureHandle) -> Self {
        value.0
    }
}

impl CustomPaintCtx<'_> {
    pub(crate) fn new<'a>(renderer: &'a mut VelloRenderer) -> CustomPaintCtx<'a> {
        CustomPaintCtx { renderer }
    }

    pub fn register_texture(&mut self, texture: Texture) -> TextureHandle {
        self.renderer.register_texture(texture).into()
    }

    pub fn unregister_texture(&mut self, handle: TextureHandle) {
        self.renderer.unregister_texture(handle.0);
    }
}
