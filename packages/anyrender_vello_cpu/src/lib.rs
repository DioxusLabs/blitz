//! An Anyrender backend using the vello_cpu crate
mod debug;
mod image_renderer;
mod scene;
mod window_renderer;

pub use image_renderer::VelloCpuImageRenderer;
pub use scene::VelloCpuScenePainter;

#[cfg(any(
    feature = "pixels_window_renderer",
    feature = "softbuffer_window_renderer"
))]
pub use window_renderer::*;

#[cfg(feature = "external")]
#[allow(clippy::single_component_path_imports, reason = "false positive")]
use vello_cpu;

#[cfg(feature = "vendored")]
mod vendored;
#[cfg(feature = "vendored")]
use vendored::{vello_api, vello_common, vello_cpu};
#[cfg(feature = "vendored")]
extern crate alloc;
