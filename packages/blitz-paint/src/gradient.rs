use crate::color::ToColorColor;
use color::{ColorSpaceTag, HueDirection};
use kurbo::{self, Affine, Point, Rect, Vec2};
use peniko::{self, ColorStop, Gradient, LinearGradientPosition, SweepGradientPosition};
use style::color::{
    AbsoluteColor, ColorSpace,
    mix::{ColorInterpolationMethod, HueInterpolationMethod},
};
use style::{
    OwnedSlice,
    values::{
        computed::{
            Angle, AngleOrPercentage, CSSPixelLength, Gradient as StyloGradient, LengthPercentage,
            LineDirection, Percentage,
        },
        generics::{
            NonNegative,
            color::GenericColor,
            image::{
                EndingShape, GenericCircle, GenericEllipse, GenericEndingShape, GenericGradient,
                GenericGradientItem, GradientFlags, ShapeExtent,
            },
            position::GenericPosition,
        },
        specified::{
            percentage::ToPercentage,
            position::{HorizontalPositionKeyword, VerticalPositionKeyword},
        },
    },
};

type GradientItem<T> = GenericGradientItem<GenericColor<Percentage>, T>;
type LinearGradient<'a> = (
    &'a LineDirection,
    &'a [GradientItem<LengthPercentage>],
    GradientFlags,
);
type RadialGradient<'a> = (
    &'a EndingShape<NonNegative<CSSPixelLength>, NonNegative<LengthPercentage>>,
    &'a GenericPosition<LengthPercentage, LengthPercentage>,
    &'a OwnedSlice<GenericGradientItem<GenericColor<Percentage>, LengthPercentage>>,
    GradientFlags,
);
type ConicGradient<'a> = (
    &'a Angle,
    &'a GenericPosition<LengthPercentage, LengthPercentage>,
    &'a OwnedSlice<GenericGradientItem<GenericColor<Percentage>, AngleOrPercentage>>,
    GradientFlags,
);

pub(crate) fn to_peniko_gradient(
    gradient: &StyloGradient,
    origin_rect: Rect,
    bounding_box: Rect,
    scale: f64,
    current_color: &AbsoluteColor,
) -> (peniko::Gradient, Option<Affine>) {
    match gradient {
        // https://developer.mozilla.org/en-US/docs/Web/CSS/gradient/linear-gradient
        GenericGradient::Linear {
            direction,
            items,
            flags,
            color_interpolation_method,
            // compat_mode,
            ..
        } => linear_gradient(
            (direction, items, *flags),
            color_interpolation_method,
            origin_rect,
            bounding_box,
            scale,
            current_color,
        ),
        GenericGradient::Radial {
            shape,
            position,
            items,
            flags,
            color_interpolation_method,
            // compat_mode,
            ..
        } => radial_gradient(
            (shape, position, items, *flags),
            color_interpolation_method,
            origin_rect,
            current_color,
        ),
        GenericGradient::Conic {
            angle,
            position,
            items,
            flags,
            color_interpolation_method,
            ..
        } => conic_gradient(
            (angle, position, items, *flags),
            color_interpolation_method,
            origin_rect,
            current_color,
        ),
    }
}

/// Preserve Stylo's interpolation metadata before producing any color stops.
fn set_color_interpolation(gradient: &mut Gradient, method: &ColorInterpolationMethod) {
    gradient.interpolation_cs = match method.space {
        ColorSpace::Srgb => ColorSpaceTag::Srgb,
        ColorSpace::SrgbLinear => ColorSpaceTag::LinearSrgb,
        ColorSpace::Hsl => ColorSpaceTag::Hsl,
        ColorSpace::Hwb => ColorSpaceTag::Hwb,
        ColorSpace::Lab => ColorSpaceTag::Lab,
        ColorSpace::Lch => ColorSpaceTag::Lch,
        ColorSpace::Oklab => ColorSpaceTag::Oklab,
        ColorSpace::Oklch => ColorSpaceTag::Oklch,
        ColorSpace::DisplayP3 => ColorSpaceTag::DisplayP3,
        // color 0.3 has no linear-P3 tag. Linear RGB bases are related by a
        // linear transform, so interpolation is equivalent in linear sRGB.
        ColorSpace::DisplayP3Linear => ColorSpaceTag::LinearSrgb,
        ColorSpace::A98Rgb => ColorSpaceTag::A98Rgb,
        ColorSpace::ProphotoRgb => ColorSpaceTag::ProphotoRgb,
        ColorSpace::Rec2020 => ColorSpaceTag::Rec2020,
        ColorSpace::XyzD50 => ColorSpaceTag::XyzD50,
        ColorSpace::XyzD65 => ColorSpaceTag::XyzD65,
    };
    gradient.hue_direction = match method.hue {
        HueInterpolationMethod::Shorter => HueDirection::Shorter,
        HueInterpolationMethod::Longer => HueDirection::Longer,
        HueInterpolationMethod::Increasing => HueDirection::Increasing,
        HueInterpolationMethod::Decreasing => HueDirection::Decreasing,
        // Stylo's non-standard "specified hue" extension has no counterpart
        // in color 0.3. Keep the previous shorter-hue fallback for this mode.
        HueInterpolationMethod::Specified => HueDirection::Shorter,
    };
}

fn linear_gradient(
    gradient: LinearGradient,
    interpolation: &ColorInterpolationMethod,
    rect: Rect,
    bounding_box: Rect,
    scale: f64,
    current_color: &AbsoluteColor,
) -> (peniko::Gradient, Option<Affine>) {
    let (direction, items, flags) = gradient;

    let center = bounding_box.center();
    let (start, end) = match direction {
        LineDirection::Angle(angle) => {
            let angle = -angle.radians64() + std::f64::consts::PI;
            let offset_length =
                rect.width() / 2.0 * angle.sin().abs() + rect.height() / 2.0 * angle.cos().abs();
            let offset_vec = Vec2::new(angle.sin(), angle.cos()) * offset_length;
            (center - offset_vec, center + offset_vec)
        }
        LineDirection::Horizontal(horizontal) => {
            let start = Point::new(rect.x0, rect.y0 + rect.height() / 2.0);
            let end = Point::new(rect.x1, rect.y0 + rect.height() / 2.0);
            match horizontal {
                HorizontalPositionKeyword::Right => (start, end),
                HorizontalPositionKeyword::Left => (end, start),
            }
        }
        LineDirection::Vertical(vertical) => {
            let start = Point::new(rect.x0 + rect.width() / 2.0, rect.y0);
            let end = Point::new(rect.x0 + rect.width() / 2.0, rect.y1);
            match vertical {
                VerticalPositionKeyword::Top => (end, start),
                VerticalPositionKeyword::Bottom => (start, end),
            }
        }
        LineDirection::Corner(horizontal, vertical) => {
            let (start_x, end_x) = match horizontal {
                HorizontalPositionKeyword::Right => (rect.x0, rect.x1),
                HorizontalPositionKeyword::Left => (rect.x1, rect.x0),
            };
            let (start_y, end_y) = match vertical {
                VerticalPositionKeyword::Top => (rect.y1, rect.y0),
                VerticalPositionKeyword::Bottom => (rect.y0, rect.y1),
            };
            (Point::new(start_x, start_y), Point::new(end_x, end_y))
        }
    };

    let gradient_length = CSSPixelLength::new((start.distance(end) / scale) as f32);
    let repeating = flags.contains(GradientFlags::REPEATING);

    let mut gradient = peniko::Gradient::new_linear(start, end).with_extend(if repeating {
        peniko::Extend::Repeat
    } else {
        peniko::Extend::Pad
    });

    set_color_interpolation(&mut gradient, interpolation);

    let (first_offset, last_offset) = resolve_length_color_stops(
        current_color,
        items,
        gradient_length,
        &mut gradient,
        repeating,
    );
    if repeating && gradient.stops.len() > 1 {
        gradient.kind = peniko::GradientKind::Linear(LinearGradientPosition {
            start: start + (end - start) * first_offset as f64,
            end: end + (start - end) * (1.0 - last_offset) as f64,
        });
    }

    if !repeating && gradient.stops.len() > 1 {
        let first = f64::from(gradient.stops[0].offset.min(0.0));
        let last = f64::from(gradient.stops.last().unwrap().offset.max(1.0));
        if first < 0.0 || last > 1.0 {
            // Keep the CSS gradient line's mapping when the backend requires
            // the color stop domain to be normalized.
            let span = last - first;
            for stop in &mut *gradient.stops {
                stop.offset = ((f64::from(stop.offset) - first) / span) as f32;
            }
            let direction = end - start;
            gradient.kind = peniko::GradientKind::Linear(LinearGradientPosition {
                start: start + direction * first,
                end: start + direction * last,
            });
        }
    }

    (gradient, None)
}

fn radial_gradient(
    gradient: RadialGradient,
    interpolation: &ColorInterpolationMethod,
    rect: Rect,
    current_color: &AbsoluteColor,
) -> (peniko::Gradient, Option<Affine>) {
    let (shape, position, items, flags) = gradient;
    let repeating = flags.contains(GradientFlags::REPEATING);

    let mut gradient = peniko::Gradient::new_radial((0.0, 0.0), 1.0).with_extend(if repeating {
        peniko::Extend::Repeat
    } else {
        peniko::Extend::Pad
    });

    set_color_interpolation(&mut gradient, interpolation);

    let (width_px, height_px) = (
        resolve_gradient_length(&position.horizontal, rect.width()),
        resolve_gradient_length(&position.vertical, rect.height()),
    );

    let gradient_scale: Option<Vec2> = match shape {
        GenericEndingShape::Circle(circle) => {
            let scale = match circle {
                GenericCircle::Extent(extent) => match extent {
                    ShapeExtent::FarthestSide => width_px
                        .max(rect.width() - width_px)
                        .max(height_px.max(rect.height() - height_px)),
                    ShapeExtent::ClosestSide => width_px
                        .min(rect.width() - width_px)
                        .min(height_px.min(rect.height() - height_px)),
                    ShapeExtent::FarthestCorner => {
                        (width_px.max(rect.width() - width_px)
                            + height_px.max(rect.height() - height_px))
                            * 0.5_f64.sqrt()
                    }
                    ShapeExtent::ClosestCorner => {
                        (width_px.min(rect.width() - width_px)
                            + height_px.min(rect.height() - height_px))
                            * 0.5_f64.sqrt()
                    }
                    _ => 0.0,
                },
                GenericCircle::Radius(radius) => radius.0.px() as f64,
            };
            Some(Vec2::new(scale, scale))
        }
        GenericEndingShape::Ellipse(ellipse) => match ellipse {
            GenericEllipse::Extent(extent) => match extent {
                ShapeExtent::FarthestCorner | ShapeExtent::FarthestSide => {
                    let mut scale = Vec2::new(
                        width_px.max(rect.width() - width_px),
                        height_px.max(rect.height() - height_px),
                    );
                    if *extent == ShapeExtent::FarthestCorner {
                        scale *= 2.0_f64.sqrt();
                    }
                    Some(scale)
                }
                ShapeExtent::ClosestCorner | ShapeExtent::ClosestSide => {
                    let mut scale = Vec2::new(
                        width_px.min(rect.width() - width_px),
                        height_px.min(rect.height() - height_px),
                    );
                    if *extent == ShapeExtent::ClosestCorner {
                        scale *= 2.0_f64.sqrt();
                    }
                    Some(scale)
                }
                _ => None,
            },
            GenericEllipse::Radii(x, y) => Some(Vec2::new(
                resolve_gradient_length(&x.0, rect.width()),
                resolve_gradient_length(&y.0, rect.height()),
            )),
        },
    };

    let gradient_transform = {
        // If the gradient has no valid scale, we don't need to calculate the color stops
        if let Some(gradient_scale) = gradient_scale {
            let (first_offset, last_offset) = resolve_length_color_stops(
                current_color,
                items,
                CSSPixelLength::new(gradient_scale.x as f32),
                &mut gradient,
                repeating,
            );
            let scale = if repeating && gradient.stops.len() >= 2 {
                (last_offset - first_offset) as f64
            } else {
                1.0
            };
            Some(
                Affine::scale_non_uniform(gradient_scale.x * scale, gradient_scale.y * scale)
                    .then_translate(get_translation(position, rect)),
            )
        } else {
            None
        }
    };

    (gradient, gradient_transform)
}

fn conic_gradient(
    gradient: ConicGradient,
    interpolation: &ColorInterpolationMethod,
    rect: Rect,
    current_color: &AbsoluteColor,
) -> (peniko::Gradient, Option<Affine>) {
    let (angle, position, items, flags) = gradient;

    let repeating = flags.contains(GradientFlags::REPEATING);
    let mut gradient = peniko::Gradient::new_sweep((0.0, 0.0), 0.0, std::f32::consts::PI * 2.0)
        .with_extend(if repeating {
            peniko::Extend::Repeat
        } else {
            peniko::Extend::Pad
        });

    set_color_interpolation(&mut gradient, interpolation);

    let (first_offset, last_offset) = resolve_angle_color_stops(
        current_color,
        items,
        CSSPixelLength::new(1.0),
        &mut gradient,
        repeating,
    );
    if repeating && gradient.stops.len() >= 2 {
        gradient.kind = peniko::GradientKind::Sweep(SweepGradientPosition {
            center: Point::new(0.0, 0.0),
            start_angle: std::f32::consts::PI * 2.0 * first_offset,
            end_angle: std::f32::consts::PI * 2.0 * last_offset,
        });
    }

    let gradient_transform = Some(
        Affine::rotate(angle.radians() as f64 - std::f64::consts::PI / 2.0)
            .then_translate(get_translation(position, rect)),
    );

    (gradient, gradient_transform)
}

#[inline]
fn resolve_length_color_stops(
    current_color: &AbsoluteColor,
    items: &[GradientItem<LengthPercentage>],
    gradient_length: CSSPixelLength,
    gradient: &mut Gradient,
    repeating: bool,
) -> (f32, f32) {
    resolve_color_stops(
        current_color,
        items,
        gradient_length,
        gradient,
        repeating,
        |gradient_length: CSSPixelLength, position: &LengthPercentage| -> Option<f32> {
            position
                .to_percentage_of(gradient_length)
                .map(|percentage| percentage.to_percentage().unwrap_or(0.0))
        },
    )
}

#[inline]
fn resolve_color_stops<T>(
    current_color: &AbsoluteColor,
    items: &[GradientItem<T>],
    gradient_length: CSSPixelLength,
    gradient: &mut Gradient,
    repeating: bool,
    item_resolver: impl Fn(CSSPixelLength, &T) -> Option<f32>,
) -> (f32, f32) {
    let mut hint: Option<f32> = None;
    let hue_direction = gradient.hue_direction;
    let subdivide_long_hue = matches!(hue_direction, HueDirection::Longer)
        && matches!(
            gradient.interpolation_cs,
            ColorSpaceTag::Hsl | ColorSpaceTag::Hwb | ColorSpaceTag::Lch | ColorSpaceTag::Oklch
        );
    if subdivide_long_hue {
        // Renderers may pad the gradient with copies of the endpoint colors.
        // Shorter interpolation keeps those spans solid. Subdivide the actual
        // color transitions below so they still follow the original long arc.
        gradient.hue_direction = HueDirection::Shorter;
    }

    // Resolve CSS stop fixup before adding renderer padding or hint samples.
    // Unpositioned stops are evenly spaced between their positioned neighbors.
    let mut resolved: Vec<_> = items
        .iter()
        .filter_map(|item| match item {
            GenericGradientItem::SimpleColorStop(color) => Some((
                Some(color.resolve_to_absolute(current_color).as_dynamic_color()),
                None,
            )),
            GenericGradientItem::ComplexColorStop { color, position } => {
                item_resolver(gradient_length, position).map(|offset| {
                    (
                        Some(color.resolve_to_absolute(current_color).as_dynamic_color()),
                        Some(offset),
                    )
                })
            }
            GenericGradientItem::InterpolationHint(position) => {
                item_resolver(gradient_length, position).map(|offset| (None, Some(offset)))
            }
        })
        .collect();

    if let Some((_, first)) = resolved.first_mut() {
        first.get_or_insert(0.0);
    }
    if let Some((_, last)) = resolved.last_mut() {
        last.get_or_insert(1.0);
    }
    let mut previous = None;
    for index in 0..resolved.len() {
        let Some(offset) = resolved[index].1 else {
            continue;
        };
        if let Some((left, start)) = previous {
            let offset = offset.max(start);
            let span = f64::from(offset) - f64::from(start);
            let distance = (index - left) as f64;
            for (step, (_, unpositioned)) in resolved[left + 1..index].iter_mut().enumerate() {
                *unpositioned =
                    Some((f64::from(start) + span * (step + 1) as f64 / distance) as f32);
            }
            resolved[index].1 = Some(offset);
        }
        previous = Some((index, resolved[index].1.unwrap()));
    }

    for (idx, (color, offset)) in resolved.into_iter().enumerate() {
        let offset = offset.unwrap();
        let Some(color) = color else {
            hint = Some(offset);
            continue;
        };

        if idx == 0 && !repeating && offset > 0.0 {
            gradient.stops.push(ColorStop { color, offset: 0.0 });
        }

        let stop = ColorStop { color, offset };
        match hint.take() {
            None => {
                if idx != 0 && subdivide_long_hue {
                    if let Some(&previous) = gradient.stops.last() {
                        append_long_hue_samples(gradient, previous, stop, 1.0);
                    }
                }
                gradient.stops.push(stop);
            }
            Some(hint) => {
                let &last_stop = gradient.stops.last().unwrap();

                if hint <= last_stop.offset {
                    gradient.stops.push(ColorStop {
                        color,
                        offset: last_stop.offset,
                    });
                    gradient.stops.push(stop);
                } else if hint >= offset {
                    gradient.stops.push(ColorStop {
                        color: last_stop.color,
                        offset,
                    });
                    gradient.stops.push(stop);
                } else if hint == (last_stop.offset + offset) / 2.0 {
                    if subdivide_long_hue {
                        append_long_hue_samples(gradient, last_stop, stop, 1.0);
                    }
                    gradient.stops.push(stop);
                } else {
                    let mid_point = (hint - last_stop.offset) / (offset - last_stop.offset);
                    let interpolator = last_stop.color.interpolate(
                        color,
                        gradient.interpolation_cs,
                        hue_direction,
                    );
                    let first_sample = gradient.stops.len();
                    let exponent = 0.5f32.log(mid_point);
                    let mut interpolate_stop = |cur_offset: f32| {
                        let relative_offset =
                            (cur_offset - last_stop.offset) / (offset - last_stop.offset);
                        let multiplier = relative_offset.powf(exponent);
                        gradient.stops.push(peniko::ColorStop {
                            color: interpolator.eval(multiplier),
                            offset: cur_offset,
                        });
                    };
                    if mid_point > 0.5 {
                        for i in 0..7 {
                            interpolate_stop(
                                last_stop.offset
                                    + (hint - last_stop.offset) * (7.0 + i as f32) / 13.0,
                            );
                        }
                        interpolate_stop(hint + (offset - hint) / 3.0);
                        interpolate_stop(hint + (offset - hint) * 2.0 / 3.0);
                    } else {
                        interpolate_stop(last_stop.offset + (hint - last_stop.offset) / 3.0);
                        interpolate_stop(last_stop.offset + (hint - last_stop.offset) * 2.0 / 3.0);
                        for i in 0..7 {
                            interpolate_stop(hint + (offset - hint) * (i as f32) / 13.0);
                        }
                    }
                    if subdivide_long_hue {
                        append_long_hue_samples(gradient, last_stop, stop, exponent);
                        gradient.stops[first_sample..]
                            .sort_by(|a, b| a.offset.total_cmp(&b.offset));
                    }
                    gradient.stops.push(stop);
                }
            }
        }
    }

    // Post-process the stops for repeating gradients
    if repeating && gradient.stops.len() > 1 {
        let first_offset = gradient.stops.first().unwrap().offset;
        let last_offset = gradient.stops.last().unwrap().offset;
        if first_offset != 0.0 || last_offset != 1.0 {
            let scale_inv = 1e-7_f32.max(1.0 / (last_offset - first_offset));
            for stop in &mut *gradient.stops {
                stop.offset = (stop.offset - first_offset) * scale_inv;
            }
        }
        (first_offset, last_offset)
    } else {
        // Ensure that the gradient ends at offset 1.0
        if gradient.stops.len() > 1 {
            let last_stop = &gradient.stops.last().unwrap();
            if last_stop.offset < 1.0 {
                let last_stop = ColorStop {
                    offset: 1.0,
                    ..(**last_stop)
                };
                gradient.stops.push(last_stop);
            }
        }
        (0.0, 1.0)
    }
}

/// Ensure that every renderer segment follows less than half a hue turn,
/// even when a color hint maps evenly spaced colors to uneven offsets.
fn append_long_hue_samples(
    gradient: &mut Gradient,
    start: ColorStop,
    end: ColorStop,
    exponent: f32,
) {
    if end.offset <= start.offset {
        return;
    }
    let interpolator =
        start
            .color
            .interpolate(end.color, gradient.interpolation_cs, HueDirection::Longer);
    for fraction in [0.25_f32, 0.5, 0.75] {
        let offset = start.offset + (end.offset - start.offset) * fraction.powf(exponent.recip());
        if start.offset < offset && offset < end.offset {
            gradient.stops.push(ColorStop {
                color: interpolator.eval(fraction),
                offset,
            });
        }
    }
}

#[inline]
fn resolve_angle_color_stops(
    current_color: &AbsoluteColor,
    items: &[GradientItem<AngleOrPercentage>],
    gradient_length: CSSPixelLength,
    gradient: &mut Gradient,
    repeating: bool,
) -> (f32, f32) {
    resolve_color_stops(
        current_color,
        items,
        gradient_length,
        gradient,
        repeating,
        |_gradient_length: CSSPixelLength, position: &AngleOrPercentage| -> Option<f32> {
            match position {
                AngleOrPercentage::Angle(angle) => {
                    Some(angle.radians() / (std::f64::consts::PI * 2.0) as f32)
                }
                AngleOrPercentage::Percentage(percentage) => {
                    Some(percentage.to_percentage().unwrap_or(0.0))
                }
            }
        },
    )
}

#[inline]
fn resolve_gradient_length(value: &LengthPercentage, basis: f64) -> f64 {
    // Percentage resolution can overflow even though computed lengths are finite.
    value
        .resolve(CSSPixelLength::new(basis as f32))
        .px()
        .clamp(f32::MIN, f32::MAX) as f64
}

#[inline]
fn get_translation(
    position: &GenericPosition<LengthPercentage, LengthPercentage>,
    rect: Rect,
) -> Vec2 {
    Vec2::new(
        rect.x0 + resolve_gradient_length(&position.horizontal, rect.width()),
        rect.y0 + resolve_gradient_length(&position.vertical, rect.height()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_percentage_overflow_is_clamped() {
        for percentage in [f32::MAX, f32::MIN] {
            let value = LengthPercentage::new_percent(Percentage(percentage));
            assert_eq!(resolve_gradient_length(&value, 300.0), percentage as f64);
        }
    }

    #[test]
    fn finite_gradient_lengths_are_preserved() {
        let length = LengthPercentage::new_length(CSSPixelLength::new(-12.0));
        let percentage = LengthPercentage::new_percent(Percentage(-0.5));
        assert_eq!(resolve_gradient_length(&length, 300.0), -12.0);
        assert_eq!(resolve_gradient_length(&percentage, 300.0), -150.0);
    }
}
