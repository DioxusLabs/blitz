use blitz_dom::util::ToColorColor as _;
use style::color::AbsoluteColor;
pub(crate) use style::computed_values::filter::single_value::T as StyloFilter;

use anyrender::filters::{Filter, FilterEffect};

pub(crate) fn convert_filters(filters: &[StyloFilter], scale: f64) -> Option<Filter> {
    if filters.is_empty() {
        return None;
    }

    Some(Filter::linear_list(
        filters
            .iter()
            .filter_map(|filter| convert_single_filter(filter, scale)),
    ))
}

pub(crate) fn convert_single_filter(filter: &StyloFilter, scale: f64) -> Option<FilterEffect> {
    // Filter lengths use the same device-pixel coordinate space as painted geometry.
    let px = |value: f32| (value as f64 * scale) as f32;
    Some(match filter {
        StyloFilter::Blur(radius) => FilterEffect::blur(px(radius.px())),
        StyloFilter::Brightness(amount) => FilterEffect::brightness(amount.0),
        StyloFilter::Contrast(amount) => FilterEffect::contrast(amount.0),
        StyloFilter::Grayscale(amount) => FilterEffect::grayscale(amount.0),
        StyloFilter::HueRotate(angle) => FilterEffect::hue_rotate(angle.radians()),
        StyloFilter::Invert(amount) => FilterEffect::invert(amount.0),
        StyloFilter::Opacity(amount) => FilterEffect::opacity(amount.0),
        StyloFilter::Saturate(amount) => FilterEffect::saturate(amount.0),
        StyloFilter::Sepia(amount) => FilterEffect::sepia(amount.0),
        StyloFilter::DropShadow(shadow) => FilterEffect::drop_shadow(
            px(shadow.horizontal.px()),
            px(shadow.vertical.px()),
            px(shadow.blur.px()),
            // TODO: pass in correct currentColor
            shadow
                .color
                .resolve_to_absolute(&AbsoluteColor::BLACK)
                .as_color_color(),
        ),
        StyloFilter::Url(_) => return None,
    })
}
