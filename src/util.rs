use parcel_css::properties::border::BorderSideWidth;
use parcel_css::values::calc::{Calc, MathFunction};
use parcel_css::values::color::CssColor;
use parcel_css::values::length::{Length, LengthValue};
use parcel_css::values::percentage::DimensionPercentage;
use piet_wgpu::Color;
use taffy::prelude::Size;

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) enum Axis {
    X,
    Y,
    // the smallest axis
    Min,
    // the largest axis
    Max,
}

pub(crate) fn translate_color(color: &CssColor) -> Color {
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

pub(crate) trait Resolve {
    fn resolve(&self, axis: Axis, rect: &Size<f32>, viewport_size: &Size<u32>) -> f64;
}

impl<T: Resolve> Resolve for Calc<T> {
    fn resolve(&self, axis: Axis, rect: &Size<f32>, viewport_size: &Size<u32>) -> f64 {
        match self {
            parcel_css::values::calc::Calc::Value(v) => v.resolve(axis, rect, viewport_size),
            parcel_css::values::calc::Calc::Number(px) => *px as f64,
            parcel_css::values::calc::Calc::Sum(v1, v2) => {
                v1.resolve(axis, rect, viewport_size) + v2.resolve(axis, rect, viewport_size)
            }
            parcel_css::values::calc::Calc::Product(v1, v2) => {
                *v1 as f64 * v2.resolve(axis, rect, viewport_size)
            }
            parcel_css::values::calc::Calc::Function(f) => f.resolve(axis, rect, viewport_size),
        }
    }
}

impl<T: Resolve> Resolve for MathFunction<T> {
    fn resolve(&self, axis: Axis, rect: &Size<f32>, viewport_size: &Size<u32>) -> f64 {
        match self {
            parcel_css::values::calc::MathFunction::Calc(c) => c.resolve(axis, rect, viewport_size),
            parcel_css::values::calc::MathFunction::Min(v) => v
                .iter()
                .map(|v| v.resolve(axis, rect, viewport_size))
                .min_by(|f1, f2| f1.partial_cmp(f2).unwrap())
                .unwrap(),
            parcel_css::values::calc::MathFunction::Max(v) => v
                .iter()
                .map(|v| v.resolve(axis, rect, viewport_size))
                .max_by(|f1, f2| f1.partial_cmp(f2).unwrap())
                .unwrap(),
            parcel_css::values::calc::MathFunction::Clamp(min, val, max) => min
                .resolve(axis, rect, viewport_size)
                .max(val.resolve(axis, rect, viewport_size).min(max.resolve(
                    axis,
                    rect,
                    viewport_size,
                ))),
            _ => todo!(),
        }
    }
}

impl Resolve for BorderSideWidth {
    fn resolve(&self, axis: Axis, rect: &Size<f32>, viewport_size: &Size<u32>) -> f64 {
        match self {
            BorderSideWidth::Thin => 2.0,
            BorderSideWidth::Medium => 4.0,
            BorderSideWidth::Thick => 6.0,
            BorderSideWidth::Length(l) => l.resolve(axis, rect, viewport_size),
        }
    }
}

impl Resolve for LengthValue {
    fn resolve(&self, _axis: Axis, _rect: &Size<f32>, viewport_size: &Size<u32>) -> f64 {
        use parcel_css::values::length::LengthValue::*;
        match self {
            Px(px) => *px as f64,
            Vw(vw) => *vw as f64 * viewport_size.width as f64 / 100.0,
            Vh(vh) => *vh as f64 * viewport_size.height as f64 / 100.0,
            Vmin(vmin) => {
                *vmin as f64 * viewport_size.height.min(viewport_size.width) as f64 / 100.0
            }
            Vmax(vmax) => {
                *vmax as f64 * viewport_size.height.max(viewport_size.width) as f64 / 100.0
            }
            _ => todo!(),
        }
    }
}

impl Resolve for Length {
    fn resolve(&self, axis: Axis, rect: &Size<f32>, viewport_size: &Size<u32>) -> f64 {
        match self {
            Length::Value(l) => l.resolve(axis, rect, viewport_size),
            Length::Calc(c) => c.resolve(axis, rect, viewport_size),
        }
    }
}

impl<T: Resolve> Resolve for DimensionPercentage<T> {
    fn resolve(&self, axis: Axis, rect: &Size<f32>, viewport_size: &Size<u32>) -> f64 {
        match self {
            DimensionPercentage::Dimension(v) => v.resolve(axis, rect, viewport_size),
            DimensionPercentage::Percentage(p) => match axis {
                Axis::X => (rect.width * p.0).into(),
                Axis::Y => (rect.height * p.0).into(),
                Axis::Min => (rect.width.min(rect.height) * p.0).into(),
                Axis::Max => (rect.width.max(rect.height) * p.0).into(),
            },
            DimensionPercentage::Calc(c) => c.resolve(axis, rect, viewport_size),
        }
    }
}
