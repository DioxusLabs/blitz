use std::rc::Rc;
use std::sync::Arc;
use std::{any::Any, cell::RefCell};

use anyrender::{RenderContext, WindowRenderer};

#[cfg(any(
    feature = "vello",
    all(
        not(feature = "alt-renderer"),
        not(all(target_os = "ios", target_abi = "sim"))
    )
))]
pub use anyrender_vello::{
    VelloRendererOptions, VelloWindowRenderer as InnerRenderer,
    wgpu::{Features, Limits},
};

#[cfg(any(
    feature = "vello-cpu-base",
    all(
        not(feature = "alt-renderer"),
        all(target_os = "ios", target_abi = "sim")
    )
))]
use anyrender_vello_cpu::VelloCpuWindowRenderer as InnerRenderer;

#[cfg(feature = "vello-hybrid")]
use anyrender_vello_hybrid::VelloHybridWindowRenderer as InnerRenderer;

#[cfg(feature = "skia")]
use anyrender_skia::SkiaWindowRenderer as InnerRenderer;

#[derive(Clone)]
pub struct DioxusNativeWindowRenderer {
    inner: Rc<RefCell<InnerRenderer>>,
}

impl Default for DioxusNativeWindowRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl DioxusNativeWindowRenderer {
    pub fn new() -> Self {
        let vello_renderer = InnerRenderer::new();
        Self::with_inner_renderer(vello_renderer)
    }

    #[cfg(any(
        feature = "vello",
        all(
            not(feature = "alt-renderer"),
            not(all(target_os = "ios", target_abi = "sim"))
        )
    ))]
    pub fn with_features_and_limits(features: Option<Features>, limits: Option<Limits>) -> Self {
        let vello_renderer = InnerRenderer::with_options(VelloRendererOptions {
            features,
            limits,
            ..Default::default()
        });
        Self::with_inner_renderer(vello_renderer)
    }

    fn with_inner_renderer(vello_renderer: InnerRenderer) -> Self {
        Self {
            inner: Rc::new(RefCell::new(vello_renderer)),
        }
    }
}

impl RenderContext for DioxusNativeWindowRenderer {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn Any>,
    ) -> Result<anyrender::ResourceId, anyrender::RegisterResourceError> {
        self.inner
            .borrow_mut()
            .try_register_custom_resource(resource)
    }

    fn unregister_resource(&mut self, resource_id: anyrender::ResourceId) {
        self.inner.borrow_mut().unregister_resource(resource_id)
    }

    fn renderer_specific_context(&self) -> Option<Box<dyn Any>> {
        self.inner.borrow_mut().renderer_specific_context()
    }
}
impl WindowRenderer for DioxusNativeWindowRenderer {
    type ScenePainter<'a>
        = <InnerRenderer as WindowRenderer>::ScenePainter<'a>
    where
        Self: 'a;

    fn resume(&mut self, window: Arc<dyn anyrender::WindowHandle>, width: u32, height: u32) {
        self.inner.borrow_mut().resume(window, width, height)
    }

    fn suspend(&mut self) {
        self.inner.borrow_mut().suspend()
    }

    fn is_active(&self) -> bool {
        self.inner.borrow().is_active()
    }

    fn set_size(&mut self, width: u32, height: u32) {
        self.inner.borrow_mut().set_size(width, height)
    }

    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F) {
        self.inner.borrow_mut().render(draw_fn)
    }
}
