use blitz_dom::util::ToColorColor as _;
use style::color::AbsoluteColor;
pub(crate) use style::computed_values::filter::single_value::T as StyloFilter;
pub(crate) use style::properties::generated::longhands::text_shadow::computed_value::T as StyloTextShadows;
use style::values::computed::SimpleShadow as StyloSimpleShadow;

use anyrender::filters::{Filter, FilterEffect};

pub(crate) fn convert_filters(filters: &[StyloFilter]) -> Option<Filter> {
    if filters.is_empty() {
        return None;
    }

    Some(Filter::linear_list(
        filters.iter().filter_map(convert_single_filter),
    ))
}

pub(crate) fn convert_text_shadows(
    shadows: &StyloTextShadows,
    current_color: &AbsoluteColor,
    scale: f32,
) -> Option<Filter> {
    if shadows.0.is_empty() {
        return None;
    }

    Some(Filter::linear_list(shadows.0.iter().map(|shadow| {
        convert_simple_shadow(shadow, current_color, scale)
    })))
}

fn convert_simple_shadow(
    shadow: &StyloSimpleShadow,
    current_color: &AbsoluteColor,
    scale: f32,
) -> FilterEffect {
    FilterEffect::drop_shadow(
        shadow.horizontal.px() * scale,
        shadow.vertical.px() * scale,
        shadow.blur.px() * scale,
        shadow
            .color
            .resolve_to_absolute(current_color)
            .as_color_color(),
    )
}

pub(crate) fn convert_single_filter(filter: &StyloFilter) -> Option<FilterEffect> {
    Some(match filter {
        StyloFilter::Blur(radius) => FilterEffect::blur(radius.px()),
        StyloFilter::Brightness(amount) => FilterEffect::brightness(amount.0),
        StyloFilter::Contrast(amount) => FilterEffect::contrast(amount.0),
        StyloFilter::Grayscale(amount) => FilterEffect::grayscale(amount.0),
        StyloFilter::HueRotate(angle) => FilterEffect::hue_rotate(angle.radians()),
        StyloFilter::Invert(amount) => FilterEffect::invert(amount.0),
        StyloFilter::Opacity(amount) => FilterEffect::opacity(amount.0),
        StyloFilter::Saturate(amount) => FilterEffect::saturate(amount.0),
        StyloFilter::Sepia(amount) => FilterEffect::sepia(amount.0),
        StyloFilter::DropShadow(shadow) => {
            // TODO: pass in correct currentColor
            convert_simple_shadow(shadow, &AbsoluteColor::BLACK, 1.0)
        }
        StyloFilter::Url(_) => return None,
    })
}
