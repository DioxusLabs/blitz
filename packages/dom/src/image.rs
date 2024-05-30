use taffy::{MaybeMath, MaybeResolve};

#[derive(Debug, Clone, Copy)]
pub struct ImageContext {
    pub width: f32,
    pub height: f32,
}

pub fn image_measure_function(
    known_dimensions: taffy::Size<Option<f32>>,
    parent_size: taffy::Size<Option<f32>>,
    image_context: &ImageContext,
    style: &taffy::Style,
) -> taffy::geometry::Size<f32> {
    // Use aspect_ratio from style, fall back to inherent aspect ratio
    let s_aspect_ratio = style.aspect_ratio;
    let aspect_ratio = s_aspect_ratio.unwrap_or_else(|| image_context.width / image_context.height);

    // Resolve sizes
    let style_size = style.size.maybe_resolve(parent_size).maybe_apply_aspect_ratio(Some(aspect_ratio));
    let min_size = style.min_size.maybe_resolve(parent_size).maybe_apply_aspect_ratio(Some(aspect_ratio));
    let max_size = style.min_size.maybe_resolve(parent_size).maybe_apply_aspect_ratio(Some(aspect_ratio));
    let inherent_size = taffy::Size {
        width: image_context.width,
        height: image_context.height,
    };


    if known_dimensions.width.is_some() | known_dimensions.height.is_some() {
        return known_dimensions.maybe_apply_aspect_ratio(Some(aspect_ratio)).map(|s| s.unwrap());
    }

    if style_size.width.is_some() | style_size.height.is_some() {
        return style_size.maybe_clamp(min_size, max_size).maybe_apply_aspect_ratio(Some(aspect_ratio)).map(|s| s.unwrap());
    }

   inherent_size.maybe_clamp(min_size, max_size)//.maybe_apply_aspect_ratio(Some(aspect_ratio)).map(|s| s.unwrap());
}
