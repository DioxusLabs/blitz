//! Paint a [`blitz_dom::BaseDocument`] by pushing [`anyrender`] drawing commands into
//! an impl [`anyrender::PaintScene`].

mod color;
mod debug_overlay;
mod gradient;
mod layers;
mod multicolor_rounded_rect;
mod non_uniform_rounded_rect;
mod render;
mod sizing;
mod text;

pub use render::paint_scene;
