//! A generic painter for blitz-dom using anyrender

mod color;
mod debug_overlay;
mod layers;
mod multicolor_rounded_rect;
mod render;
mod text;

pub use render::{BlitzDomPainter, paint_scene};
