//! A [`vello`] backend for the [`anyrender`] 2D drawing abstraction
mod image_renderer;
mod scene;
mod window_renderer;

pub mod custom_paint_source;

pub use custom_paint_source::*;
pub use image_renderer::VelloImageRenderer;
pub use scene::VelloScenePainter;
pub use window_renderer::{VelloRendererOptions, VelloWindowRenderer};

pub use wgpu;

use std::num::NonZeroUsize;

#[cfg(target_os = "macos")]
const DEFAULT_THREADS: Option<NonZeroUsize> = NonZeroUsize::new(1);
#[cfg(not(target_os = "macos"))]
const DEFAULT_THREADS: Option<NonZeroUsize> = None;
