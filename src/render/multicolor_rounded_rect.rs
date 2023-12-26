//! A rounded rect closer to the browser
//! Implemented in such a way that splits the border into 4 parts at the midway of each radius
//!
//! Can I just say, this is a lot of work for a border
//! HTML/css is annoyingly wild

use std::{
    f64::consts::FRAC_PI_4,
    f64::consts::PI,
    f64::consts::{FRAC_PI_2, FRAC_PI_8},
};

use dioxus::prelude::SvgAttributes;
use euclid::Length;
use style::{
    properties::{longhands::width, style_structs::Border},
    values::computed::CSSPixelLength,
};
use vello::kurbo::{
    Arc, ArcAppendIter, BezPath, CubicBez, Ellipse, PathEl, Point, Rect, RoundedRect,
    RoundedRectRadii, Shape, Vec2,
};

use crate::Document;

pub struct SplitRoundedRect {
    pub rect: Rect,
    pub tl_radius: Vec2,
    pub tr_radius: Vec2,
    pub bl_radius: Vec2,
    pub br_radius: Vec2,
}

pub struct RectArcs {
    pub top: [Arc; 2],
    pub right: [Arc; 2],
    pub bottom: [Arc; 2],
    pub left: [Arc; 2],
}

pub enum BorderSide {
    Inside,
    Outside,
    Inline,
}

impl SplitRoundedRect {
    // Split a rounded rect up into propery slices
    pub fn new(rect: Rect, border: &Border) -> Self {
        todo!()
        // Self { rect }
    }

    #[rustfmt::skip]
    pub fn arcs(
        &self,
        side: BorderSide,
        top_width: f64,
        right_width: f64,
        bottom_width: f64,
        left_width: f64,
    ) -> RectArcs {

        todo!();

        // let width = self.rect.width();
        // let height = self.rect.height();


        // let inv = match side {
        //     BorderSide::Inside => 1.0,
        //     BorderSide::Outside => -1.0,
        //     BorderSide::Inline => 0.0,
        // };

        // // note that we need to adjust radii since
        // RectArcs {
        //     top: [
        //         self.arc(-FRAC_PI_4, tl, tl , tl - top_width * inv / 2.0 ),  // start at top left (mid -> end)
        //         self.arc(0.0, width - tr, tr, tr - top_width * inv / 2.0), // jump to top right arc and (start -> mid)
        //     ],
        //     right: [
        //         self.arc(FRAC_PI_4, width - tr , tr, tr - right_width * inv / 2.0), // jump to top right arc and (start -> mid)
        //         self.arc(FRAC_PI_2, width - br, height - br, br - right_width * inv / 2.0), // jump to top right arc and (start -> mid)
        //     ],
        //     bottom: [
        //         self.arc(FRAC_PI_2 + FRAC_PI_4, width - br, height - br, br - bottom_width * inv / 2.0), // jump to top right arc and (start -> mid)
        //         self.arc(PI, bl, height - bl, bl - bottom_width * inv / 2.0), // jump to top right arc and (start -> mid)
        //     ],
        //     left: [
        //         self.arc(PI + FRAC_PI_4, bl, height - bl, bl - left_width * inv / 2.0), // jump to top right arc and (start -> mid)
        //         self.arc(PI + FRAC_PI_2, tl, tl, tl - left_width * inv / 2.0), // jump to top right arc and (start -> mid)
        //     ],
        // }
    }

    pub fn arc(&self, start_angle: f64, x_offset: f64, y_offset: f64, radius: f64) -> Arc {
        Arc {
            // For whatever reason, kurbo starts 0 and the x origin and rotates clockwise
            // Mentally I think of it as starting at the y origin (unit circle)
            x_rotation: PI + FRAC_PI_2,
            center: Point {
                x: self.rect.x0 + x_offset,
                y: self.rect.y0 + y_offset,
            },
            radii: Vec2 {
                x: radius,
                y: radius,
            },
            start_angle,
            sweep_angle: FRAC_PI_4,
        }
    }
}

#[rustfmt::skip]
pub fn manual_elements() -> Vec<PathEl> {
    // let mut path = Path::new();
    // path.move_to((0.0, 0.0));

    // pth
    vec![
        PathEl::MoveTo(Point { x: 100.0, y: 1000.0 }),
        PathEl::LineTo(Point { x: 100.0, y: 500.0 }),
        PathEl::LineTo(Point { x: 100.0, y: 400.0 }),
        PathEl::LineTo(Point { x: 200.0, y: 500.0 }),
        PathEl::LineTo(Point { x: 200.0, y: 1000.0 }),
        PathEl::ClosePath,
    ]
}

impl Document {
    #[rustfmt::skip]
    pub fn top_segment(&self, rect: Rect, border: &Border, tolerance: f64) -> BezPath {
        // please pay a smart person to simplify this

        let Border {
            border_top_width,
            border_right_width,
            border_left_width,
            border_top_left_radius,
            border_top_right_radius,
            ..
        } = border;

        let scale = self.viewport.scale_f64();

        // Resolve the radii to a length. need to downscale since the radii are in document pixels
        let pixel_width = CSSPixelLength::new((rect.width() / scale) as _);
        let pixel_height = CSSPixelLength::new((rect.height() / scale) as _);

        // Resolve and rescale
        // We have to scale since document pixels are not same same as rendered pixels
        let border_top_width = scale * border_top_width.to_f64_px();
        let border_left_width = scale * border_left_width.to_f64_px();
        let border_right_width = scale * border_right_width.to_f64_px();

        let border_top_left_radius_width = scale * border_top_left_radius.0.width.0.resolve(pixel_width).px() as f64;
        let border_top_left_radius_height = scale * border_top_left_radius.0.height.0.resolve(pixel_height).px() as f64;
        let border_top_right_radius_width = scale * border_top_right_radius.0.width.0.resolve(pixel_width).px() as f64;
        let border_top_right_radius_height = scale * border_top_right_radius.0.height.0.resolve(pixel_height).px() as f64;

        let Rect { x0, y0, x1, .. } = rect;

        let mut path = BezPath::new();

        // 1. Top left corner
        // If no radius, just kinda draw a trapezoid
        if border_top_left_radius_width == 0.0 || border_top_left_radius_height == 0.0 {
            // inner corner
            path.move_to(Point { x: x0 + border_left_width, y: y0 + border_top_width });

            // outer corner
            path.line_to(Point { x: x0, y: y0 });
        } else {
            // If a radius is present, calculate the center of the arcs
            // The inner and outer arc will share a center: the center of the outer arc
            let outer_radii = Vec2 { x: border_top_left_radius_width, y: border_top_left_radius_height };
            let inner_radii = Vec2 { x: border_top_left_radius_width - border_left_width, y: border_top_left_radius_height - border_top_width };
            let center = rect.origin() + outer_radii;

            // if the radius is bigger than the border, we need to draw the inner arc to fill in the gap
            if border_top_left_radius_width > border_left_width || border_top_left_radius_height > border_top_width {
                let angle_start = solve_start_angle_for_border(border_top_width, border_left_width, inner_radii);
                path.insert_arc(
                    Arc::new(center, inner_radii,  PI + FRAC_PI_2, - (FRAC_PI_2 - angle_start), 0.0),
                    tolerance
                );
            } else {
                path.move_to(Point { x: x0 + border_left_width, y: y0 + border_top_width });
            }

            // Draw the outer arc
            let angle_start = solve_start_angle_for_border(border_top_width, border_left_width, outer_radii);
            path.insert_arc(
                Arc::new(center, outer_radii,  PI + angle_start, FRAC_PI_2 - angle_start, 0.0),
                tolerance
            );
        }


        // 2. Top right corner
        // If no radius, just draw a line
        if border_top_right_radius_width == 0.0 || border_top_right_radius_height == 0.0 {
            // outer corner
            path.line_to(Point { x: x1, y: y0 });

            // inner corner
            path.line_to(Point { x: x1 - border_left_width, y: y0 + border_top_width });
        } else {
            // If a radius is present, calculate the center of the arcs
            // The inner and outer arc will share a center: the center of the outer arc
            let outer_radii = Vec2 { x: border_top_right_radius_width, y: border_top_right_radius_height };
            let inner_radii = Vec2 { x: border_top_right_radius_width - border_right_width, y: border_top_right_radius_height - border_top_width };
            let center = rect.origin() + Vec2 { x: rect.width() - outer_radii.x, y: outer_radii.y } ;

            // Draw the outer arc
            let angle_start = solve_start_angle_for_border(border_top_width, border_right_width, outer_radii);
            path.insert_arc(
                Arc::new(center, outer_radii,  PI + FRAC_PI_2,  FRAC_PI_2-angle_start, 0.0),
                tolerance
            );

            // // if the radius is bigger than the border, we need to draw the inner arc to fill in the gap
            if border_top_right_radius_width > border_right_width || border_top_right_radius_height > border_top_width {
                let angle_start = solve_start_angle_for_border(border_top_width, border_right_width, inner_radii);
                path.insert_arc(
                    Arc::new(center, inner_radii,  -angle_start,  -(FRAC_PI_2- angle_start) , 0.0),
                    tolerance
                );
            } else {
                path.line_to(Point { x: x1 - border_right_width, y: y0 + border_top_width });
            }
        }

        path
    }
}

struct RadiiPair {
    inner: Vec2,
    outer: Vec2,
    center: Point,
    corner: Corner,
}

impl RadiiPair {
    fn new(center: Point, corner: Corner, border: &Border) -> Self {
        match corner {
            Corner::TopLeft => todo!(),
            Corner::TopRight => todo!(),
            Corner::BottomLeft => todo!(),
            Corner::BottomRight => todo!(),
        }

        let inner = todo!();
        let outer = todo!();

        Self {
            inner,
            outer,
            center,
            corner,
        }
    }
}

enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

trait InsertArc {
    fn insert_arc(&mut self, arc: Arc, tolerance: f64);
}

impl InsertArc for BezPath {
    fn insert_arc(&mut self, arc: Arc, tolerance: f64) {
        let mut elements = arc.path_elements(tolerance);
        match elements.next().unwrap() {
            PathEl::MoveTo(a) if self.elements().len() > 0 => self.push(PathEl::LineTo(a)),
            el => self.push(el),
        }
        self.extend(elements)
    }
}

fn solve_start_angle_for_border(bt_width: f64, br_width: f64, radii: Vec2) -> f64 {
    // slope of the border intersection split
    let w = bt_width / br_width;
    let x = radii.y / (w * radii.x);

    maybe_simple_theta(x)
}

/// Use a quick solution to find the start sweep angle of the border curve using the border width
fn maybe_simple_theta(x: f64) -> f64 {
    /*
    Any point on an ellipse is given by:
    x = a cos(t)
    y = b sin(t)

    The equation of the border intersect is:
    y = w (x - a) + b

    where w is the ratio of the width to the height of the border
    and b is the y intercept of the border
    and x is the x intercept of the border

    This formula is the result of solving the system of equations:
    x = a cos(t)
    y = b sin(t)
    y = w (x - a) + b

    b/(w*a) = (cos(t) - 1)/(sin(t) - 1)

    The solution to the system of equations is:
    https://www.wolframalpha.com/input?i=%28cos%28x%29-1%29%2F%28sin%28x%29-1%29+%3D+a+solve+for+x
    */

    use std::f64::consts::SQRT_2;
    let numerator: f64 = x - x.sqrt() * SQRT_2;
    let denonimantor: f64 = x - 2.0;
    (numerator / denonimantor).atan() * 2.0
}

#[test]
fn should_solve_properly() {
    // 0.643501
    dbg!(maybe_simple_theta(0.5));

    dbg!(solve_start_angle_for_border(
        4.0,
        1.0,
        Vec2 { x: 1.0, y: 2.0 }
    ));
}
