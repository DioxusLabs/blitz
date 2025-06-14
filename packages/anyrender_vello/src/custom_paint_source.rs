use std::sync::Arc;

use peniko::Blob;
use vello::Renderer as VelloRenderer;
use vello::peniko::Image;
use wgpu::{Device, Queue, TexelCopyTextureInfoBase, Texture};

pub trait CustomPaintSource: 'static {
    fn resume(&mut self, device: &Device, queue: &Queue);
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

#[derive(Copy, Clone, PartialEq, Hash)]
pub struct TextureHandle {
    pub(crate) id: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl TextureHandle {
    pub(crate) fn dummy_image(&self) -> Image {
        dummy_image(Some(self.id), self.width, self.height)
    }
}

impl CustomPaintCtx<'_> {
    pub(crate) fn new<'a>(renderer: &'a mut VelloRenderer) -> CustomPaintCtx<'a> {
        CustomPaintCtx { renderer }
    }

    pub fn register_texture(&mut self, texture: Texture) -> TextureHandle {
        let dummy_image = dummy_image(None, texture.width(), texture.height());
        let handle = TextureHandle {
            id: dummy_image.data.id(),
            width: texture.width(),
            height: texture.height(),
        };
        let base = TexelCopyTextureInfoBase {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        };
        self.renderer.override_image(&dummy_image, Some(base));

        handle
    }

    pub fn unregister_texture(&mut self, handle: TextureHandle) {
        let dummy_image = dummy_image(Some(handle.id), handle.width, handle.height);
        self.renderer.override_image(&dummy_image, None);
    }
}

// Everything except blob id, width, and height is ignored
fn dummy_image(id: Option<u64>, width: u32, height: u32) -> Image {
    let blob = match id {
        Some(id) => Blob::from_raw_parts(Arc::new([]), id),
        None => Blob::new(Arc::new([])),
    };

    Image {
        data: blob,
        width,
        height,
        format: vello::peniko::ImageFormat::Rgba8,
        x_extend: vello::peniko::Extend::Pad,
        y_extend: vello::peniko::Extend::Pad,
        quality: vello::peniko::ImageQuality::High,
        alpha: 1.0,
    }
}
