//! First-party inline SVG rendering, behind the `svg-native` feature flag

pub mod attrs;
pub mod construct;
pub mod context;
pub mod geometry;
pub mod hit_test;
pub mod resolve;
pub mod text;
pub mod viewport;

pub use construct::{construct_svg_fragment, rebuild_svg_fragments};
pub use context::{
    Align, MeetOrSlice, PreserveAspectRatio, SvgContext, SvgNode, SvgNodeKind, TextRun,
};
