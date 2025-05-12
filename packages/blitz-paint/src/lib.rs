//! A generic painter for blitz-dom using anyrender

mod debug_overlay;
mod layers;
mod multicolor_rounded_rect;
mod render;
mod text;
mod util;

pub use render::{BlitzDomPainter, paint_scene};
