//! Translate a blitz-dom into [`anyrender`] drawing commands

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
