mod build;
mod floats;
mod layout;
mod lines;

use taffy::{AvailableSpace, LayoutInput, Point, Rect, Size};

pub(crate) use build::push_inline_content;
pub(crate) use layout::{float_box_inputs, inline_box_inputs};

/// What `compute_inline_layout_inner` has resolved from the container's styles and inputs
/// before the inline boxes are measured and the lines broken.
pub(crate) struct Frame {
    pub(crate) inputs: LayoutInput,
    pub(crate) node_size: Size<Option<f32>>,
    pub(crate) node_min_size: Size<Option<f32>>,
    pub(crate) node_max_size: Size<Option<f32>>,
    pub(crate) aspect_ratio: Option<f32>,
    pub(crate) padding: Rect<f32>,
    pub(crate) border: Rect<f32>,
    pub(crate) scrollbar_gutter: Point<f32>,
    pub(crate) container_pb: Rect<f32>,
    pub(crate) content_box_inset: Rect<f32>,
    pub(crate) child_inputs: LayoutInput,
    pub(crate) available_space: Size<AvailableSpace>,
    pub(crate) collapses_through: bool,
    pub(crate) suppresses_baseline: bool,
    pub(crate) scale: f32,
}
