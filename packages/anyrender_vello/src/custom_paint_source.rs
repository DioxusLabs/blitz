use crate::wgpu_context::DeviceHandle;
use peniko::ImageData;
use vello::Renderer as VelloRenderer;
use wgpu::{Instance, TexelCopyTextureInfoBase, Texture};

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
        let dummy_image = self.renderer.register_texture(texture.clone());

        let base = TexelCopyTextureInfoBase {
            texture: texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        };
        self.renderer.override_image(&dummy_image, Some(base));

        TextureHandle(dummy_image)
    }

    pub fn unregister_texture(&mut self, handle: TextureHandle) {
        self.renderer.override_image(&handle.0, None);
    }
}
