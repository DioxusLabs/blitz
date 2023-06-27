use lightningcss::properties::border::BorderSideWidth;
use lightningcss::values;
use lightningcss::values::angle::Angle;
use lightningcss::values::position::{
    HorizontalPositionKeyword, PositionComponent, VerticalPositionKeyword,
};
use taffy::prelude::Size;
use values::calc::{Calc, MathFunction};
use values::color::CssColor;
use values::length::{Length, LengthValue};
use values::percentage::DimensionPercentage;
use vello::peniko::Color;

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
    let rgb = color.to_rgb().unwrap();
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
            values::calc::Calc::Value(v) => v.resolve(axis, rect, viewport_size),
            values::calc::Calc::Number(px) => *px as f64,
            values::calc::Calc::Sum(v1, v2) => {
                v1.resolve(axis, rect, viewport_size) + v2.resolve(axis, rect, viewport_size)
            }
            values::calc::Calc::Product(v1, v2) => {
                *v1 as f64 * v2.resolve(axis, rect, viewport_size)
            }
            values::calc::Calc::Function(f) => f.resolve(axis, rect, viewport_size),
        }
    }
}

impl<T: Resolve> Resolve for MathFunction<T> {
    fn resolve(&self, axis: Axis, rect: &Size<f32>, viewport_size: &Size<u32>) -> f64 {
        match self {
            values::calc::MathFunction::Calc(c) => c.resolve(axis, rect, viewport_size),
            values::calc::MathFunction::Min(v) => v
                .iter()
                .map(|v| v.resolve(axis, rect, viewport_size))
                .min_by(|f1, f2| f1.partial_cmp(f2).unwrap())
                .unwrap(),
            values::calc::MathFunction::Max(v) => v
                .iter()
                .map(|v| v.resolve(axis, rect, viewport_size))
                .max_by(|f1, f2| f1.partial_cmp(f2).unwrap())
                .unwrap(),
            values::calc::MathFunction::Clamp(min, val, max) => min
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
        use values::length::LengthValue::*;
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

impl Resolve for PositionComponent<HorizontalPositionKeyword> {
    fn resolve(&self, axis: Axis, rect: &Size<f32>, viewport_size: &Size<u32>) -> f64 {
        match self {
            PositionComponent::Center => rect.width as f64 / 2.0,
            PositionComponent::Length(l) => l.resolve(axis, rect, viewport_size),
            PositionComponent::Side { side, offset } => {
                let offset = offset
                    .as_ref()
                    .map(|offset| offset.resolve(axis, rect, viewport_size))
                    .unwrap_or_default();
                match side {
                    HorizontalPositionKeyword::Left => offset,
                    HorizontalPositionKeyword::Right => rect.width as f64 - offset,
                }
            }
        }
    }
}

impl Resolve for PositionComponent<VerticalPositionKeyword> {
    fn resolve(&self, axis: Axis, rect: &Size<f32>, viewport_size: &Size<u32>) -> f64 {
        match self {
            PositionComponent::Center => rect.height as f64 / 2.0,
            PositionComponent::Length(l) => l.resolve(axis, rect, viewport_size),
            PositionComponent::Side { side, offset } => {
                let offset = offset
                    .as_ref()
                    .map(|offset| offset.resolve(axis, rect, viewport_size))
                    .unwrap_or_default();
                match side {
                    VerticalPositionKeyword::Bottom => offset,
                    VerticalPositionKeyword::Top => rect.height as f64 - offset,
                }
            }
        }
    }
}

pub(crate) fn map_dimension_percentage<A, B>(
    input: DimensionPercentage<A>,
    f: impl Fn(A) -> B + Copy,
) -> DimensionPercentage<B> {
    match input {
        DimensionPercentage::Dimension(v) => DimensionPercentage::Dimension(f(v)),
        DimensionPercentage::Percentage(p) => DimensionPercentage::Percentage(p),
        DimensionPercentage::Calc(c) => {
            // This is used to avoid recursively creating an unique type for the closure used in map_calc
            fn map_dimension_percentage_calc<A, B>(
                input: Calc<DimensionPercentage<A>>,
                f: impl Fn(A) -> B + Copy,
            ) -> Calc<DimensionPercentage<B>> {
                match input {
                    Calc::Value(v) => Calc::Value(Box::new(map_dimension_percentage(*v, f))),
                    Calc::Number(n) => Calc::Number(n),
                    Calc::Sum(v1, v2) => Calc::Sum(
                        Box::new(map_dimension_percentage_calc(*v1, f)),
                        Box::new(map_dimension_percentage_calc(*v2, f)),
                    ),
                    Calc::Product(v1, v2) => {
                        Calc::Product(v1, Box::new(map_dimension_percentage_calc(*v2, f)))
                    }
                    Calc::Function(_) => todo!(),
                }
            }
            DimensionPercentage::Calc(Box::new(map_dimension_percentage_calc(*c, f)))
        }
    }
}

#[allow(unused)]
pub(crate) fn map_calc<A, B>(input: Calc<A>, f: impl Fn(A) -> B) -> Calc<B> {
    match input {
        Calc::Value(v) => Calc::Value(Box::new(f(*v))),
        Calc::Number(n) => Calc::Number(n),
        Calc::Sum(v1, v2) => Calc::Sum(Box::new(map_calc(*v1, &f)), Box::new(map_calc(*v2, f))),
        Calc::Product(v1, v2) => Calc::Product(v1, Box::new(map_calc(*v2, &f))),
        Calc::Function(_) => todo!(),
    }
}

pub trait AngleExt {
    fn to_turn_percentage(&self) -> f32;
}

impl AngleExt for Angle {
    fn to_turn_percentage(&self) -> f32 {
        self.to_degrees() / 360.0
    }
}
