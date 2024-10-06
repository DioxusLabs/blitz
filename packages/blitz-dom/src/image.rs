use taffy::{MaybeMath, MaybeResolve};

#[derive(Debug, Clone, Copy)]
pub struct ImageContext {
    pub inherent_size: taffy::Size<f32>,
    pub attr_size: taffy::Size<Option<f32>>,
}

pub fn image_measure_function(
    known_dimensions: taffy::Size<Option<f32>>,
    parent_size: taffy::Size<Option<f32>>,
    image_context: &ImageContext,
    style: &taffy::Style,
    _debug: bool,
) -> taffy::geometry::Size<f32> {
    let inherent_size = image_context.inherent_size;

    // Use aspect_ratio from style, fall back to inherent aspect ratio
    let s_aspect_ratio = style.aspect_ratio;
    let aspect_ratio = s_aspect_ratio.unwrap_or_else(|| inherent_size.width / inherent_size.height);

    // Resolve sizes
    let style_size = style
        .size
        .maybe_resolve(parent_size)
        .maybe_apply_aspect_ratio(Some(aspect_ratio));
    let min_size = style
        .min_size
        .maybe_resolve(parent_size)
        .maybe_apply_aspect_ratio(Some(aspect_ratio));
    let max_size = style
        .max_size
        .maybe_resolve(parent_size)
        .maybe_apply_aspect_ratio(Some(aspect_ratio));
    let attr_size = image_context
        .attr_size
        .maybe_apply_aspect_ratio(Some(aspect_ratio));

    if known_dimensions.width.is_some() | known_dimensions.height.is_some() {
        return known_dimensions
            .maybe_apply_aspect_ratio(Some(aspect_ratio))
            .map(|s| s.unwrap());
    }

    if style_size.width.is_some() | style_size.height.is_some() {
        return style_size
            .maybe_clamp(min_size, max_size)
            .maybe_apply_aspect_ratio(Some(aspect_ratio))
            .map(|s| s.unwrap());
    }

    if attr_size.width.is_some() | attr_size.height.is_some() {
        return attr_size
            .maybe_clamp(min_size, max_size)
            .maybe_apply_aspect_ratio(Some(aspect_ratio))
            .map(|s| s.unwrap());
    }

    inherent_size
        .maybe_clamp(min_size, max_size)
        .map(Some)
        .maybe_apply_aspect_ratio(Some(aspect_ratio))
        .map(|s| s.unwrap())
}
