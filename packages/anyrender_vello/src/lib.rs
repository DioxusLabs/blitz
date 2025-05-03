//! An Anyrender backend using the vello crate
mod image_renderer;
mod scene;
mod window_renderer;

pub use image_renderer::VelloImageRenderer;
pub use scene::VelloAnyrenderScene;
pub use window_renderer::VelloWindowRenderer;

use std::num::NonZeroUsize;

#[cfg(target_os = "macos")]
const DEFAULT_THREADS: Option<NonZeroUsize> = NonZeroUsize::new(1);
#[cfg(not(target_os = "macos"))]
const DEFAULT_THREADS: Option<NonZeroUsize> = None;
