//! An Anyrender backend using the vello_cpu crate
mod debug;
mod image_renderer;
mod scene;
mod window_renderer;

pub use image_renderer::VelloCpuImageRenderer;
pub use scene::VelloCpuScenePainter;
pub use window_renderer::*;

use crate::debug::DebugTimer;

#[cfg(feature = "external")]
#[allow(clippy::single_component_path_imports, reason = "false positive")]
use vello_cpu;

#[cfg(feature = "vendored")]
mod vendored;
#[cfg(feature = "vendored")]
use vendored::{vello_api, vello_common, vello_cpu};
#[cfg(feature = "vendored")]
extern crate alloc;
