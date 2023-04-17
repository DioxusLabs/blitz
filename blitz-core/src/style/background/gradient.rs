use lightningcss::values::gradient;
use lightningcss::values::gradient::GradientItem;
use lightningcss::values::length::LengthPercentage;
use lightningcss::values::length::LengthValue;
use lightningcss::values::position::HorizontalPositionKeyword;
use lightningcss::values::position::VerticalPositionKeyword;
use smallvec::SmallVec;
use std::f64::consts::PI;
use std::sync::Arc;
use std::sync::Mutex;
use taffy::prelude::Size;
use vello::kurbo::Affine;
use vello::kurbo::Point;
use vello::kurbo::Shape;
use vello::peniko;
use vello::peniko::Color;
use vello::peniko::Extend;
use vello::SceneBuilder;

use crate::util::map_dimension_percentage;
use crate::util::translate_color;
use crate::util::AngleExt;
use crate::util::Resolve;

use super::linear_gradient::LinearGradient;
use super::radial_gradient::RadialGradient;
use super::Repeat;

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum GradientType {
    Linear(LinearGradient),
    Radial(RadialGradient),
    Conic,
}

impl<'a> From<&'a gradient::LinearGradient> for GradientType {
    fn from(l: &'a gradient::LinearGradient) -> Self {
        GradientType::Linear(LinearGradient::new(match &l.direction {
            gradient::LineDirection::Angle(angle) => angle.to_radians().into(),
            gradient::LineDirection::Horizontal(HorizontalPositionKeyword::Left) => PI * 1.5,
            gradient::LineDirection::Horizontal(HorizontalPositionKeyword::Right) => PI * 0.5,
            gradient::LineDirection::Vertical(VerticalPositionKeyword::Top) => 0.0,
            gradient::LineDirection::Vertical(VerticalPositionKeyword::Bottom) => PI * 1.0,
            _ => todo!(),
        }))
    }
}

impl From<gradient::RadialGradient> for GradientType {
    fn from(r: gradient::RadialGradient) -> Self {
        GradientType::Radial(RadialGradient {
            position: r.position,
            shape: match r.shape {
                gradient::EndingShape::Circle(circle) => circle,
                _ => gradient::Circle::Extent(gradient::ShapeExtent::FarthestSide),
            },
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct ColorStop {
    color: Color,
    position: Option<LengthPercentage>,
}

impl TryFrom<&GradientItem<LengthPercentage>> for ColorStop {
    type Error = ();

    fn try_from(item: &GradientItem<LengthPercentage>) -> Result<Self, Self::Error> {
        match item {
            GradientItem::ColorStop(color_stop) => Ok(ColorStop {
                color: translate_color(&color_stop.color),
                position: color_stop.position.clone(),
            }),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Gradient {
    gradient_type: GradientType,
    stops: Vec<ColorStop>,
    // The last size and resolved positions and stops. This is used cache the last resolved position and stops so that we don't have to resolve them again if they are the same.
    last_resolved: Mutex<Option<GradientCache>>,
    repeating: bool,
}

impl TryFrom<gradient::Gradient> for Gradient {
    type Error = ();

    fn try_from(gradient: gradient::Gradient) -> Result<Self, Self::Error> {
        use gradient::Gradient::*;
        let (gradient_type, repeating) = match &gradient {
            Linear(liniar) => (liniar.into(), false),
            RepeatingLinear(liniar) => (liniar.into(), true),
            Radial(radial) => (radial.clone().into(), false),
            RepeatingRadial(radial) => (radial.clone().into(), true),
            Conic(_) => (GradientType::Conic, false),
            RepeatingConic(_) => (GradientType::Conic, true),
            _ => return Err(()),
        };
        let stops = match gradient {
            Linear(linear) | RepeatingLinear(linear) => linear.items,
            Radial(radial) | RepeatingRadial(radial) => radial.items,
            Conic(conic) | RepeatingConic(conic) => conic
                .items
                .into_iter()
                .map(|item| match item {
                    GradientItem::ColorStop(stop) => GradientItem::ColorStop(gradient::ColorStop {
                        color: stop.color,
                        position: stop.position.map(|percentage| {
                            map_dimension_percentage(percentage, |angle| {
                                LengthValue::Px(angle.to_turn_percentage())
                            })
                        }),
                    }),
                    GradientItem::Hint(pos) => {
                        GradientItem::Hint(map_dimension_percentage(pos, |angle| {
                            LengthValue::Px(angle.to_turn_percentage())
                        }))
                    }
                })
                .collect(),
            _ => return Err(()),
        };

        let stops = stops
            .into_iter()
            .map(|stop| ColorStop::try_from(&stop))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Gradient {
            gradient_type,
            stops,
            last_resolved: Default::default(),
            repeating,
        })
    }
}

#[derive(Clone, Debug)]
struct GradientCache {
    rect: Size<f32>,
    viewport_size: Size<u32>,
    result: Arc<peniko::ColorStops>,
}

impl PartialEq for Gradient {
    fn eq(&self, other: &Self) -> bool {
        self.gradient_type == other.gradient_type && self.stops == other.stops
    }
}

impl Gradient {
    /// Resolve all missing positions. If a position is missing, it is in between the last resolved position and the next resolved position.
    pub(crate) fn resolve_stops(
        &self,
        rect: &Size<f32>,
        viewport_size: &Size<u32>,
    ) -> Arc<peniko::ColorStops> {
        let mut last_resolved = self.last_resolved.lock().unwrap();
        let resolved = last_resolved
            .as_ref()
            .filter(|last_resolved| {
                last_resolved.rect == *rect && last_resolved.viewport_size == *viewport_size
            })
            .is_some();
        if resolved {
            last_resolved.as_ref().unwrap().result.clone()
        } else {
            let mut resolved = smallvec::SmallVec::default();

            let mut last_resolved_position = None;
            let mut unresolved_positions: Vec<Color> = Vec::new();

            let mut resolve_pos =
                |position: Option<f32>,
                 resolved: &mut SmallVec<[peniko::ColorStop; 4]>,
                 unresolved_positions: &mut Vec<Color>| {
                    let resolved_position = position.unwrap_or(1.);
                    let total_unresolved = unresolved_positions.len();
                    for (i, unresolved_stop) in unresolved_positions.iter_mut().enumerate() {
                        let last_pos = last_resolved_position.unwrap_or_default();

                        let exclude_last_pos = last_resolved_position.is_some();
                        let exclude_next_pos = position.is_some();

                        let offset = last_pos
                            + (((i + exclude_last_pos as usize) as f32)
                                / (total_unresolved + (exclude_next_pos as usize) * 2 - 1) as f32)
                                * (resolved_position - last_pos);

                        resolved.push(peniko::ColorStop {
                            color: *unresolved_stop,
                            offset,
                        });
                    }
                    unresolved_positions.clear();
                    last_resolved_position = Some(resolved_position);
                    resolved_position
                };

            for stop in &self.stops {
                match &stop.position {
                    Some(position) => {
                        let position =
                            position.resolve(crate::util::Axis::X, rect, viewport_size) as f32;
                        resolve_pos(Some(position), &mut resolved, &mut unresolved_positions);
                        resolved.push(peniko::ColorStop {
                            color: stop.color,
                            offset: position,
                        });
                    }
                    None => {
                        unresolved_positions.push(stop.color);
                    }
                }
            }

            // resolve any remaining positions
            resolve_pos(None, &mut resolved, &mut unresolved_positions);

            *last_resolved = Some(GradientCache {
                rect: *rect,
                viewport_size: *viewport_size,
                result: Arc::new(resolved),
            });

            last_resolved.as_ref().unwrap().result.clone()
        }
    }

    pub(crate) fn render(
        &self,
        sb: &mut SceneBuilder,
        shape: &impl Shape,
        repeat: Repeat,
        rect: &Size<f32>,
        viewport_size: &Size<u32>,
    ) {
        let stops = self.resolve_stops(rect, viewport_size);
        let extend = if self.repeating {
            Extend::Repeat
        } else {
            repeat.into()
        };
        match &self.gradient_type {
            GradientType::Linear(gradient) => {
                let bb = shape.bounding_box();
                let starting_point_offset = gradient.center_offset(*rect);
                let ending_point_offset =
                    Point::new(-starting_point_offset.x, -starting_point_offset.y);
                let center = bb.center();
                let start = Point::new(
                    center.x + starting_point_offset.x,
                    center.y + starting_point_offset.y,
                );
                let end = Point::new(
                    center.x + ending_point_offset.x,
                    center.y + ending_point_offset.y,
                );

                let kind = peniko::GradientKind::Linear { start, end };

                let gradient = peniko::Gradient {
                    kind,
                    extend,
                    stops: (*stops).clone(),
                };

                let brush = peniko::BrushRef::Gradient(&gradient);

                sb.fill(peniko::Fill::NonZero, Affine::IDENTITY, brush, None, shape)
            }
            GradientType::Radial(gradient) => {
                let bb = shape.bounding_box();
                let pos_x = bb.x0
                    + gradient
                        .position
                        .x
                        .resolve(crate::util::Axis::X, rect, viewport_size);
                let pos_y = bb.y0
                    + gradient
                        .position
                        .y
                        .resolve(crate::util::Axis::Y, rect, viewport_size);
                let pos = Point::new(pos_x, pos_y);

                let end_radius = gradient.radius_in(pos, rect, viewport_size) as f32;

                let kind = peniko::GradientKind::Radial {
                    start_center: pos,
                    start_radius: 0.0,
                    end_center: pos,
                    end_radius,
                };

                let gradient = peniko::Gradient {
                    kind,
                    extend,
                    stops: (*stops).clone(),
                };

                let brush = peniko::BrushRef::Gradient(&gradient);

                sb.fill(peniko::Fill::NonZero, Affine::IDENTITY, brush, None, shape)
            }
            GradientType::Conic => todo!(),
        }
    }
}
