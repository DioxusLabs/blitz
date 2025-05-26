//! A generic painter for blitz-dom using anyrender

mod color;
mod debug_overlay;
mod gradient;
mod layers;
mod multicolor_rounded_rect;
mod non_uniform_rounded_rect;
mod render;
mod sizing;
mod text;

pub use render::{BlitzDomPainter, paint_scene};
