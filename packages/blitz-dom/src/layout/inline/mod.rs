mod build;
mod collapse;
mod layout;
mod offset_map;

pub(crate) use build::build_inline_layout_into;
pub(crate) use collapse::is_segment_break;
pub use offset_map::{Affinity, TextOffsetMap};
