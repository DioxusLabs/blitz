use markup5ever::{LocalName, local_name};
use taffy::{
    AvailableSpace, Baselines, BoxSizing, CollapsibleMarginSet, CoreStyle, LayoutInput,
    LayoutOutput, MaybeMath, MaybeResolve, RequestedAxis, ResolveOrZero as _, RunMode, Size,
    SizingMode,
};

/// Whether an element is a replaced element laid out as a leaf box with an
/// intrinsic size. Note: `<object>` is deliberately excluded as its fallback
/// children should render when no resource is loaded.
pub(crate) fn is_replaced_element(tag_name: &LocalName) -> bool {
    *tag_name == local_name!("img")
        || *tag_name == local_name!("svg")
        || *tag_name == local_name!("canvas")
        || *tag_name == local_name!("video")
        || *tag_name == local_name!("embed")
        || *tag_name == local_name!("iframe")
}

/// The intrinsic dimensions of a replaced element per CSS Images 3
/// (https://drafts.csswg.org/css-images/#intrinsic-dimensions): an intrinsic
/// width, an intrinsic height, and an intrinsic aspect ratio, each of which
/// may independently be absent.
#[derive(Debug, Clone, Copy, Default)]
pub struct IntrinsicSizes {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub ratio: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
pub struct ReplacedContext {
    /// The element's intrinsic dimensions, each possibly absent.
    pub intrinsic_sizes: IntrinsicSizes,
    /// The default object size (https://drafts.csswg.org/css-images/#default-object-size)
    /// used to fill in dimensions the intrinsic sizes leave unresolved. 300x150
    /// for most replaced elements; zero for an image with no loaded resource.
    pub default_object_size: taffy::Size<f32>,
}

/// Builds a [`LayoutOutput`] for a replaced element of the given border-box size.
/// Replaced elements have no content that overflows, no baselines, and never
/// collapse margins through themselves.
fn layout_output_from_size(size: Size<f32>) -> LayoutOutput {
    LayoutOutput {
        size,
        content_size: size,
        baselines: Baselines::NONE,
        top_margin: CollapsibleMarginSet::ZERO,
        bottom_margin: CollapsibleMarginSet::ZERO,
        margins_can_collapse_through: false,
    }
}

/// Whether a height/width value is violating it's min- and max- constraints
/// The min- and max- constraints cannot both be violated because the max
/// constraint if floored by the min constraint (min constraint takes priority)
enum Violation {
    /// Constraints are not violated
    None,
    /// Min constraint is violated
    Min,
    /// Max constraint is violated
    Max,
}

pub fn compute_replaced_layout(
    inputs: LayoutInput,
    style: &impl CoreStyle,
    resolve_calc_value: impl Fn(*const (), f32) -> f32,
    context: &ReplacedContext,
) -> LayoutOutput {
    let LayoutInput {
        known_dimensions,
        parent_size,
        available_space,
        sizing_mode,
        run_mode,
        axis: requested_axis,
        ..
    } = inputs;

    let padding = style
        .padding()
        .resolve_or_zero(parent_size.width, &resolve_calc_value);
    let border = style
        .border()
        .resolve_or_zero(parent_size.width, &resolve_calc_value);
    let padding_border = padding + border;
    let pb_sum = Size {
        width: padding_border.left + padding_border.right,
        height: padding_border.top + padding_border.bottom,
    };
    let box_sizing_adjustment = if style.box_sizing() == BoxSizing::BorderBox {
        pb_sum
    } else {
        Size::ZERO
    };

    // Return early if both width and height are known
    if run_mode == RunMode::ComputeSize {
        if let Size {
            width: Some(width),
            height: Some(height),
        } = known_dimensions
        {
            return layout_output_from_size(Size {
                width: width.max(pb_sum.width),
                height: height.max(pb_sum.height),
            });
        }
    }

    // Use aspect_ratio from style, fall back to inherent aspect ratio (if any).
    //
    // A degenerate ratio -- zero, infinite, or NaN -- is discarded. Transferring
    // such a ratio between axes computes `width / ratio` or `height * ratio`,
    // which yields an infinite (ratio == 0) or NaN (ratio is NaN) size in the
    // other axis. CSS Sizing 4 4.1 says a degenerate ratio behaves as `auto`,
    // so each source is filtered *before* the fallback: a degenerate style
    // ratio falls back to the inherent one (that is what `auto` means for a
    // replaced element), and only if both are degenerate or absent does the
    // element get no preferred aspect ratio at all.
    let is_usable_ratio = |ratio: &f32| ratio.is_finite() && *ratio > 0.0;
    let intrinsic = context.intrinsic_sizes;
    let aspect_ratio: Option<f32> = style
        .aspect_ratio()
        .filter(is_usable_ratio)
        .or(intrinsic.ratio.filter(is_usable_ratio));

    // Concrete object size per the CSS default sizing algorithm with no
    // specified size (https://drafts.csswg.org/css-images/#default-sizing):
    // missing intrinsic dimensions are derived from the aspect ratio when
    // possible, otherwise taken from the default object size. An element with
    // only an intrinsic aspect ratio uses the stretch-fit width in normal flow
    // (CSS2 §10.3.2) when the available width is definite; shrink-to-fit
    // contexts (min/max-content) contain it within the default object size.
    //
    // Only the intrinsic ratio participates here: the `aspect-ratio` property
    // affects the sizing of the box, not the natural dimensions of its content.
    let intrinsic_ratio = intrinsic.ratio.filter(is_usable_ratio);
    let default_size = context.default_object_size;
    let inherent_size = match (intrinsic.width, intrinsic.height) {
        (Some(w), Some(h)) => Size {
            width: w,
            height: h,
        },
        (Some(w), None) => Size {
            width: w,
            height: intrinsic_ratio
                .map(|r| w / r)
                .unwrap_or(default_size.height),
        },
        (None, Some(h)) => Size {
            width: intrinsic_ratio.map(|r| h * r).unwrap_or(default_size.width),
            height: h,
        },
        (None, None) => match intrinsic_ratio {
            Some(ratio) => {
                if let AvailableSpace::Definite(available_width) = available_space.width {
                    Size {
                        width: available_width,
                        height: available_width / ratio,
                    }
                } else {
                    // Contain within the default object size
                    let scale = (default_size.width / ratio).min(default_size.height);
                    Size {
                        width: scale * ratio,
                        height: scale,
                    }
                }
            }
            None => default_size,
        },
    };

    // See https://www.w3.org/TR/css-sizing-3/#replaced-percentage-min-contribution
    let basis_for_max_and_preferred = Size {
        width: if available_space.width == AvailableSpace::MinContent {
            Some(0.0)
        } else {
            parent_size.width
        },
        height: if available_space.height == AvailableSpace::MinContent {
            Some(0.0)
        } else {
            parent_size.height
        },
    };

    // Resolve sizes
    let style_size = style
        .size()
        .maybe_resolve(basis_for_max_and_preferred, &resolve_calc_value)
        .maybe_sub(box_sizing_adjustment);
    let mut min_size = style
        .min_size()
        .maybe_resolve(parent_size, &resolve_calc_value)
        .maybe_sub(box_sizing_adjustment);
    let max_size = style
        .max_size()
        .maybe_resolve(basis_for_max_and_preferred, &resolve_calc_value)
        .or(available_space.into_options())
        .maybe_min(available_space.into_options())
        .maybe_max(min_size)
        .maybe_sub(box_sizing_adjustment);

    // For ContentSize mode, ignore preferred/min size styles in the axis being measured: the
    // parent layout algorithm applies them itself, and content-based measurement should return
    // the content size (the intrinsic size for replaced elements). Constraints in the opposite
    // axis are retained as they transfer through the aspect ratio (transferred size suggestion).
    let mut style_size = style_size;
    if sizing_mode == SizingMode::ContentSize {
        match requested_axis {
            RequestedAxis::Horizontal => {
                style_size.width = None;
                min_size.width = None;
            }
            RequestedAxis::Vertical => {
                style_size.height = None;
                min_size.height = None;
            }
            RequestedAxis::Both => {}
        }
    }

    // Known dimensions are the parent's current sizing inputs. Clamp them before transferring
    // an aspect ratio so provisional cross-axis sizes do not bypass replaced-element limits.
    if known_dimensions.width.is_some() | known_dimensions.height.is_some() {
        // Style max sizes without the available-space fallback: available space must not
        // constrain the aspect-ratio transfer of parent-resolved dimensions.
        let style_max_size = style
            .max_size()
            .maybe_resolve(basis_for_max_and_preferred, &resolve_calc_value)
            .maybe_sub(box_sizing_adjustment)
            .maybe_max(min_size);

        let content_box_known_dimensions = known_dimensions.maybe_sub(pb_sum);
        let transferred = content_box_known_dimensions
            .maybe_clamp(min_size, style_max_size)
            .maybe_apply_aspect_ratio(aspect_ratio)
            .unwrap_or(inherent_size);

        // Known axes are authoritative (already resolved by the parent); only the axis
        // derived via aspect-ratio transfer (or falling back to the intrinsic size) is
        // clamped by this element's min/max sizes.
        let size = content_box_known_dimensions
            .unwrap_or(transferred.maybe_clamp(min_size, style_max_size));

        return layout_output_from_size(size.map(|s| s.max(0.0)) + pb_sum);
    }

    let unclamped_size = if style_size.width.is_some() | style_size.height.is_some() {
        style_size
            .maybe_apply_aspect_ratio(aspect_ratio)
            .unwrap_or(inherent_size)
    } else {
        inherent_size
    };

    // Floor size at zero
    let size = unclamped_size.map(|s| s.max(0.0));

    // Violations
    let width_violation = if size.width < min_size.width.unwrap_or(0.0) {
        Violation::Min
    } else if size.width > max_size.width.unwrap_or(f32::INFINITY) {
        Violation::Max
    } else {
        Violation::None
    };

    let height_violation = if size.height < min_size.height.unwrap_or(0.0) {
        Violation::Min
    } else if size.height > max_size.height.unwrap_or(f32::INFINITY) {
        Violation::Max
    } else {
        Violation::None
    };

    // Without an intrinsic aspect ratio, each axis is clamped independently
    let Some(aspect_ratio) = aspect_ratio else {
        let size = size.maybe_clamp(min_size, max_size);
        return layout_output_from_size(size + pb_sum);
    };
    let inv_aspect_ratio = 1.0 / aspect_ratio;

    // Clamp following rules in table at
    // https://www.w3.org/TR/CSS22/visudet.html#min-max-widths
    let size = match (width_violation, height_violation) {
        // No constraint violation
        (Violation::None, Violation::None) => size,
        // w > max-width
        (Violation::Max, Violation::None) => {
            let max_width = max_size.width.unwrap();
            Size {
                width: max_width,
                height: (max_width * inv_aspect_ratio).maybe_max(min_size.height),
            }
        }
        // w < min-width
        (Violation::Min, Violation::None) => {
            let min_width = min_size.width.unwrap();
            Size {
                width: min_width,
                height: (min_width * inv_aspect_ratio).maybe_min(max_size.height),
            }
        }
        // h > max-height
        (Violation::None, Violation::Max) => {
            let max_height = max_size.height.unwrap();
            Size {
                width: (max_height * aspect_ratio).maybe_max(min_size.width),
                height: max_height,
            }
        }
        // h < min-height
        (Violation::None, Violation::Min) => {
            let min_height = min_size.height.unwrap();
            Size {
                width: (min_height * aspect_ratio).maybe_min(max_size.width),
                height: min_height,
            }
        }
        // (w > max-width) and (h > max-height)
        (Violation::Max, Violation::Max) => {
            let max_width = max_size.width.unwrap();
            let max_height = max_size.height.unwrap();
            if max_width / size.width <= max_height / size.height {
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
        (Violation::Min, Violation::Min) => {
            let min_width = min_size.width.unwrap();
            let min_height = min_size.height.unwrap();
            if min_width / size.width <= min_height / size.height {
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
        (Violation::Min, Violation::Max) => {
            let min_width = min_size.width.unwrap();
            let max_height = max_size.height.unwrap();
            Size {
                width: min_width,
                height: max_height,
            }
        }
        // (w < min-width) and (h > max-height)
        (Violation::Max, Violation::Min) => {
            let max_width = max_size.width.unwrap();
            let min_height = min_size.height.unwrap();
            Size {
                width: max_width,
                height: min_height,
            }
        }
    };

    layout_output_from_size(size + pb_sum)
}
