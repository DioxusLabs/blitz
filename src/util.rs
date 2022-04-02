use parcel_css::values::calc::{Calc, MathFunction};
use parcel_css::values::color::CssColor;
use parcel_css::values::length::LengthValue;
use parcel_css::values::percentage::DimensionPercentage;
use piet_wgpu::Color;
use stretch2::prelude::Size;

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum Axis {
    X,
    Y,
    // the smallest axis
    Min,
    // the largest axis
    Max,
}

pub fn translate_color(color: &CssColor) -> Color {
    let rgb = color.to_rgb();
    if let CssColor::RGBA(rgba) = rgb {
        Color::rgba(
            rgba.red as f64 / 255.0,
            rgba.green as f64 / 255.0,
            rgba.blue as f64 / 255.0,
            rgba.alpha as f64 / 255.0,
        )
    } else {
        panic!("translation failed");
    }
}

/// Resolve a mesaurement to a number of pixels.
pub fn resolve_measure(
    measure: &DimensionPercentage<LengthValue>,
    axis: Axis,
    rect: &Size<f32>,
    viewport_size: &Size<f32>,
) -> f64 {
    match measure {
        DimensionPercentage::Dimension(v) => resolve_length(v, axis, viewport_size),
        DimensionPercentage::Percentage(p) => match axis {
            Axis::X => (rect.width * p.0).into(),
            Axis::Y => (rect.height * p.0).into(),
            Axis::Min => (rect.width.min(rect.height) * p.0).into(),
            Axis::Max => (rect.width.max(rect.height) * p.0).into(),
        },
        DimensionPercentage::Calc(c) => resolve_calcuation(c, axis, rect, viewport_size),
    }
}

pub fn resolve_calcuation(
    calc: &Calc<DimensionPercentage<LengthValue>>,
    axis: Axis,
    rect: &Size<f32>,
    viewport_size: &Size<f32>,
) -> f64 {
    match calc {
        parcel_css::values::calc::Calc::Value(v) => resolve_measure(&v, axis, rect, viewport_size),
        parcel_css::values::calc::Calc::Number(px) => *px as f64,
        parcel_css::values::calc::Calc::Sum(v1, v2) => {
            resolve_calcuation(v1, axis, rect, viewport_size)
                + resolve_calcuation(v2, axis, rect, viewport_size)
        }
        parcel_css::values::calc::Calc::Product(v1, v2) => {
            *v1 as f64 * resolve_calcuation(v2, axis, rect, viewport_size)
        }
        parcel_css::values::calc::Calc::Function(f) => {
            resolve_function(f, axis, rect, viewport_size)
        }
    }
}

pub fn resolve_function(
    func: &MathFunction<DimensionPercentage<LengthValue>>,
    axis: Axis,
    rect: &Size<f32>,
    viewport_size: &Size<f32>,
) -> f64 {
    match func {
        parcel_css::values::calc::MathFunction::Calc(c) => {
            resolve_calcuation(c, axis, rect, viewport_size)
        }
        parcel_css::values::calc::MathFunction::Min(v) => v
            .iter()
            .map(|v| resolve_calcuation(v, axis, rect, viewport_size))
            .min_by(|f1, f2| f1.partial_cmp(f2).unwrap())
            .unwrap(),
        parcel_css::values::calc::MathFunction::Max(v) => v
            .iter()
            .map(|v| resolve_calcuation(v, axis, rect, viewport_size))
            .max_by(|f1, f2| f1.partial_cmp(f2).unwrap())
            .unwrap(),
        parcel_css::values::calc::MathFunction::Clamp(min, val, max) => {
            resolve_calcuation(min, axis, rect, viewport_size).max(
                resolve_calcuation(val, axis, rect, viewport_size).min(resolve_calcuation(
                    max,
                    axis,
                    rect,
                    viewport_size,
                )),
            )
        }
    }
}

pub fn resolve_length(length: &LengthValue, _axis: Axis, _viewport_size: &Size<f32>) -> f64 {
    use parcel_css::values::length::LengthValue::*;
    match length {
        Px(px) => *px as f64,
        _ => todo!(),
    }
}
