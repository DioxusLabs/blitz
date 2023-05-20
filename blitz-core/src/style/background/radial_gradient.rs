use crate::util::Resolve;
use lightningcss::values::gradient;
use lightningcss::values::position::Position;
use taffy::prelude::Size;
use vello::kurbo::Point;

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct RadialGradient {
    pub position: Position,
    // TODO: Vello doesn't support non-circle gradients yet
    pub shape: lightningcss::values::gradient::Circle,
}

impl RadialGradient {
    pub fn radius_in(&self, position: Point, rect: &Size<f32>, viewport_width: &Size<u32>) -> f64 {
        fn distance_to_nearest_side(pos: f64, size: f64) -> f64 {
            (pos.min(size - pos)).abs()
        }
        fn distance_to_farthest_side(pos: f64, size: f64) -> f64 {
            (pos.max(size - pos)).abs()
        }
        match &self.shape {
            gradient::Circle::Extent(gradient::ShapeExtent::ClosestSide) => {
                distance_to_nearest_side(position.x, rect.width as f64)
                    .min(distance_to_nearest_side(position.y, rect.height as f64))
            }
            gradient::Circle::Extent(gradient::ShapeExtent::FarthestSide) => {
                distance_to_farthest_side(position.x, rect.width as f64)
                    .max(distance_to_farthest_side(position.y, rect.height as f64))
            }
            gradient::Circle::Extent(gradient::ShapeExtent::ClosestCorner) => {
                let points = [
                    Point::new(0., 0.),
                    Point::new(rect.width as f64, 0.),
                    Point::new(0., rect.height as f64),
                    Point::new(rect.width as f64, rect.height as f64),
                ];
                points
                    .iter()
                    .map(|p| p.distance(position))
                    .min_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap()
            }
            gradient::Circle::Extent(gradient::ShapeExtent::FarthestCorner) => {
                let points = [
                    Point::new(0., 0.),
                    Point::new(rect.width as f64, 0.),
                    Point::new(0., rect.height as f64),
                    Point::new(rect.width as f64, rect.height as f64),
                ];
                points
                    .iter()
                    .map(|p| p.distance(position))
                    .max_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap()
            }
            gradient::Circle::Radius(radius) => {
                radius.resolve(crate::util::Axis::X, rect, viewport_width)
            }
        }
    }
}
