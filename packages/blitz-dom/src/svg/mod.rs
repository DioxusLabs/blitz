//! First-party inline `<svg>` rendering, behind the `svg-native` feature.

pub(crate) mod attrs;
pub(crate) mod construct;
mod context;
pub(crate) mod viewport;

pub use construct::rebuild_svg_fragments;
pub use context::SvgContext;
