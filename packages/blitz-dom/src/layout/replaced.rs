use taffy::{BoxSizing, CoreStyle as _, MaybeMath, MaybeResolve, ResolveOrZero as _, Size};

use crate::layout::resolve_calc_value;

#[derive(Debug, Clone, Copy)]
pub struct ReplacedContext {
    pub inherent_size: taffy::Size<f32>,
    pub attr_size: taffy::Size<Option<f32>>,
}

pub fn replaced_measure_function(
    known_dimensions: taffy::Size<Option<f32>>,
    parent_size: taffy::Size<Option<f32>>,
    image_context: &ReplacedContext,
    style: &taffy::Style,
    _debug: bool,
) -> taffy::Size<f32> {
    let inherent_size = image_context.inherent_size;

    let padding = style
        .padding()
        .resolve_or_zero(parent_size.width, resolve_calc_value);
    let border = style
        .border()
        .resolve_or_zero(parent_size.width, resolve_calc_value);
    let padding_border = padding + border;
    let pb_sum = Size {
        width: padding_border.left + padding_border.right,
        height: padding_border.top + padding_border.bottom,
    };
    let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox {
        pb_sum
    } else {
        Size::ZERO
    };

    // Use aspect_ratio from style, fall back to inherent aspect ratio
    let s_aspect_ratio = style.aspect_ratio;
    let aspect_ratio = s_aspect_ratio.unwrap_or_else(|| inherent_size.width / inherent_size.height);
    let inv_aspect_ratio = 1.0 / aspect_ratio;

    // Resolve sizes
    let style_size = style
        .size
        .maybe_resolve(parent_size, resolve_calc_value)
        .maybe_apply_aspect_ratio(Some(aspect_ratio))
        .maybe_sub(box_sizing_adjustment);
    let min_size = style
        .min_size
        .maybe_resolve(parent_size, resolve_calc_value)
        .maybe_sub(box_sizing_adjustment);
    let max_size = style
        .max_size
        .maybe_resolve(parent_size, resolve_calc_value)
        .maybe_max(min_size)
        .maybe_sub(box_sizing_adjustment);
    let attr_size = image_context.attr_size;

    let unclamped_size = 'size: {
        if known_dimensions.width.is_some() | known_dimensions.height.is_some() {
            break 'size known_dimensions
                .maybe_apply_aspect_ratio(Some(aspect_ratio))
                .map(|s| s.unwrap());
        }

        if style_size.width.is_some() | style_size.height.is_some() {
            break 'size style_size
                // .maybe_clamp(min_size, max_size)
                .maybe_apply_aspect_ratio(Some(aspect_ratio))
                .map(|s| s.unwrap());
        }

        if attr_size.width.is_some() | attr_size.height.is_some() {
            break 'size attr_size
                // .maybe_clamp(min_size, max_size)
                .maybe_apply_aspect_ratio(Some(aspect_ratio))
                .map(|s| s.unwrap());
        }

        inherent_size
            // .maybe_clamp(min_size, max_size)
            .map(Some)
            .maybe_apply_aspect_ratio(Some(aspect_ratio))
            .map(|s| s.unwrap())
    };

    // Violations
    let w_min = unclamped_size.width < min_size.width.unwrap_or(0.0);
    let w_max = unclamped_size.width > max_size.width.unwrap_or(f32::INFINITY);
    let h_min = unclamped_size.height < min_size.height.unwrap_or(0.0);
    let h_max = unclamped_size.height > max_size.height.unwrap_or(f32::INFINITY);

    // Clamp following rules in table at
    // https://www.w3.org/TR/CSS22/visudet.html#min-max-widths
    let size = match (w_min, w_max, h_min, h_max) {
        // No constraint violation
        (false, false, false, false) => unclamped_size,
        // w > max-width
        (false, true, false, false) => {
            let max_width = max_size.width.unwrap();
            Size {
                width: max_width,
                height: (max_width * inv_aspect_ratio).maybe_max(min_size.height),
            }
        }
        // w < min-width
        (true, false, false, false) => {
            let min_width = min_size.width.unwrap();
            Size {
                width: min_width,
                height: (min_width * inv_aspect_ratio).maybe_min(max_size.height),
            }
        }
        // h > max-height
        (false, false, false, true) => {
            let max_height = max_size.height.unwrap();
            Size {
                width: (max_height * aspect_ratio).maybe_max(min_size.width),
                height: max_height,
            }
        }
        // h < min-height
        (false, false, true, false) => {
            let min_height = min_size.height.unwrap();
            Size {
                width: (min_height * aspect_ratio).maybe_min(max_size.width),
                height: min_height,
            }
        }
        // (w > max-width) and (h > max-height)
        (false, true, false, true) => {
            let max_width = max_size.width.unwrap();
            let max_height = max_size.height.unwrap();
            if max_width / unclamped_size.width <= max_height / unclamped_size.height {
                Size {
                    width: max_width,
                    height: (max_width * inv_aspect_ratio).maybe_max(min_size.height),
                }
            } else {
                Size {
                    width: (max_height * aspect_ratio).maybe_max(min_size.width),
                    height: max_height,
                }
            }
        }
        // (w < min-width) and (h < min-height)
        (true, false, true, false) => {
            let min_width = min_size.width.unwrap();
            let min_height = min_size.height.unwrap();
            if min_width / unclamped_size.width <= min_height / unclamped_size.height {
                Size {
                    width: (min_height * aspect_ratio).maybe_min(max_size.width),
                    height: min_height,
                }
            } else {
                Size {
                    width: min_width,
                    height: (min_width * inv_aspect_ratio).maybe_min(max_size.height),
                }
            }
        }
        // (w < min-width) and (h > max-height)
        (true, false, false, true) => {
            let min_width = min_size.width.unwrap();
            let max_height = max_size.height.unwrap();
            Size {
                width: min_width,
                height: max_height,
            }
        }
        // (w < min-width) and (h > max-height)
        (false, true, true, false) => {
            let max_width = max_size.width.unwrap();
            let min_height = min_size.height.unwrap();
            Size {
                width: max_width,
                height: min_height,
            }
        }

        _ => unreachable!("Max was already floored by min, so we cannot have both a min and a max violation in the same axis")
    };

    size + pb_sum
}
