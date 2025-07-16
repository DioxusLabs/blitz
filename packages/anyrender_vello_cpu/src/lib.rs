//! An Anyrender backend using the vello_cpu crate
mod image_renderer;
mod scene;

#[cfg(feature = "softbuffer_window_renderer")]
mod softbuffer_window_renderer;
#[cfg(feature = "softbuffer_window_renderer")]
pub use softbuffer_window_renderer::VelloCpuSoftbufferWindowRenderer;
#[cfg(feature = "pixels_window_renderer")]
mod pixels_window_renderer;
#[cfg(feature = "pixels_window_renderer")]
pub use pixels_window_renderer::VelloCpuPixelsWindowRenderer;

#[cfg(feature = "pixels_window_renderer")]
pub use VelloCpuPixelsWindowRenderer as VelloCpuWindowRenderer;
#[cfg(all(
    feature = "softbuffer_window_renderer",
    not(feature = "pixels_window_renderer")
))]
pub use VelloCpuSoftbufferWindowRenderer as VelloCpuWindowRenderer;

pub use image_renderer::VelloCpuImageRenderer;
pub use scene::VelloCpuScenePainter;

#[cfg(feature = "external")]
#[allow(clippy::single_component_path_imports, reason = "false positive")]
use vello_cpu;

#[cfg(feature = "vendored")]
mod vendored;
#[cfg(feature = "vendored")]
use vendored::{vello_api, vello_common, vello_cpu};
#[cfg(feature = "vendored")]
extern crate alloc;
