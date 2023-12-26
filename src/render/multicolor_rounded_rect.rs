//! A rounded rect closer to the browser
//! Implemented in such a way that splits the border into 4 parts at the midway of each radius

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
    Arc, ArcAppendIter, CubicBez, Ellipse, PathEl, Point, Rect, RoundedRect, RoundedRectRadii,
    Shape, Vec2,
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
    pub fn top_segment(&self, rect: Rect, border: &Border, tolerance: f64) -> Vec<PathEl> {
        let Border {
            border_top_width,
            border_right_width,
            border_top_left_radius,
            border_top_right_radius,
            ..
        } = border;

        let scale = self.viewport.scale_f64();

        // Resolve the radii to a length
        let pixel_width = CSSPixelLength::new(rect.width() as _);
        let pixel_height = CSSPixelLength::new(rect.width() as _);

        // Resolve and rescale
        // We have to scale since document pixels are not same same as rendered pixels
        let border_top_width = scale * border_top_width.to_f64_px();
        let border_left_width = scale * border_right_width.to_f64_px();
        let border_right_width = scale * border_right_width.to_f64_px();

        let border_top_left_radius_width = scale * border_top_left_radius.0.width.0.resolve(pixel_width).px() as f64;
        let border_top_left_radius_height = scale * border_top_left_radius.0.height.0.resolve(pixel_height).px() as f64;
        let border_top_right_radius_width = scale * border_top_right_radius.0.width.0.resolve(pixel_width).px() as f64;
        let border_top_right_radius_height = scale * border_top_right_radius.0.height.0.resolve(pixel_height).px() as f64;

        let mut path = vec![];

        /*
        Draw the





         */





        // 1. Top left corner
        // If no radius, just kinda draw a trapezoid
        if border_top_left_radius_width == 0.0 || border_top_left_radius_height == 0.0 {
            // inner corner
            path.push(PathEl::MoveTo(Point { x: rect.x0+border_left_width, y: rect.y0 + border_top_width }));

            // outer corner
            path.push(PathEl::LineTo(Point { x: rect.x0, y: rect.y0 }));
        } else {
            // If a radius is present, calculate the center of the arcs
            // The inner and outer arc will share a center: the center of the outer arc
            let outer_radii = Vec2 { x: border_top_left_radius_width, y: border_top_left_radius_height };
            let inner_radii = Vec2 { x: border_top_left_radius_width - border_left_width, y: border_top_left_radius_height - border_top_width };
            let center = rect.origin() + outer_radii;

            // if the radius is bigger than the border, we need to draw the inner arc to fill in the gap
            if border_top_left_radius_width > border_left_width || border_top_left_radius_height > border_top_width {
                // Draw the inner arc
                let angle_start = solve_start_angle_for_border(border_top_width, border_left_width, inner_radii.x, inner_radii.y);
                let angle_sweep = FRAC_PI_2 - angle_start;
                let arc = Arc::new(center, inner_radii,  PI + FRAC_PI_2, -angle_sweep, 0.0);
                push_arc(&mut path, arc, tolerance);
            } else {
                path.push(PathEl::MoveTo(Point { x: rect.x0 + border_left_width, y: rect.y0 + border_top_width }));
            }

            // Draw the outer arc
            let angle_start = solve_start_angle_for_border(border_top_width, border_left_width, outer_radii.x, outer_radii.y);
            let angle_sweep = FRAC_PI_2 - angle_start;
            let arc = Arc::new(center, outer_radii,  PI + angle_start, angle_sweep, 0.0);
            push_arc(&mut path, arc, tolerance);
        }


        // 2. Top right corner
        // If no radius, just draw a line
        // if border_top_right_radius_width == 0.0 || border_top_right_radius_height == 0.0 {
        path.push(PathEl::LineTo(Point { x: rect.x1, y: rect.y0 }));
        // } else {
        //     // Draw the rightmost outer arc
        //     let radii = Vec2 { x: border_top_right_radius_width, y: border_top_right_radius_height };
        //     let right_center = rect.origin() + Vec2 { x: rect.width() - radii.x, y: radii.y } ;

        //     let angle_start = solve_start_angle_for_border(border_top_width, border_right_width, radii.x, radii.y);
        //     let angle_sweep = FRAC_PI_2 - angle_start;
        //     let arc = Arc::new(right_center, radii, PI + FRAC_PI_2, angle_sweep, 0.0);
        //     push_arc(&mut path, arc, 0.1);



        // }

        // 3. Inner right corner
        // Check if the radius will clip the corner
        // If it won't we just draw a point
        // if border_top_right_radius_width <= border_top_width || border_top_right_radius_height <= border_top_width {
        path.push(PathEl::LineTo(Point { x: rect.x1 - border_right_width, y: rect.y0 + border_top_width }));
        // } else {
        //     // Draw the rightmost inner arc
        //     let radii = Vec2 {
        //         x: border_top_right_radius_width - border_top_width,
        //         y: border_top_right_radius_height - border_top_width,
        //     };
        //     let right_center = rect.origin() + Vec2 { x: rect.width() - radii.x, y: radii.y } ;

        //     let angle_start = solve_start_angle_for_border(border_top_width, border_right_width, radii.x, radii.y);
        //     let angle_sweep = FRAC_PI_2 - angle_start;
        //     let arc = Arc::new(right_center, radii, PI + FRAC_PI_2, angle_sweep, 0.0);
        //     push_arc(&mut path, arc, 0.1);
        // }





        // // Go to inner corner (width - border_width)
        // path.push(PathEl::LineTo(Point { x: rect.x1-border_right_width , y: rect.y0 + border_top_width }));

        // Go to other inner corner
        // path.push(PathEl::LineTo(Point { x: rect.x0+border_left_width, y: rect.y0 + border_top_width }));



        // // draw the rightmost outer arc
        // let radii = Vec2 {
        //     x: border_top_right_radius_width,
        //     y: border_top_right_radius_height,
        // };
        // let right_center = rect.origin() + Vec2 { x: rect.width(), y: 0.0 } - Vec2 { x: radii.x, y: -radii.y };
        // let angle_start = solve_start_angle_for_border(border_top_width, border_right_width, radii.x, radii.y);
        // let arc = Arc::new(right_center, radii, 0.0, angle_start, -FRAC_PI_2);
        // push_arc(&mut path, arc, tolerance);




        // // Draw the innermost arc
        // let radii = Vec2 {
        //     x: border_top_right_radius_width.px() as f64 - border_top_width,
        //     y: border_top_right_radius_height.px() as f64 - border_top_width,
        // };
        // let radii = Vec2 { x: radii.x * scale, y: radii.y * scale };
        // let arc = Arc::new(right_center, radii, 0.0, -FRAC_PI_2, 0.0);
        // push_arc(&mut path, arc, tolerance);

        // let radii = Vec2 {
        //     x: border_top_left_radius_width.px() as f64 - border_top_width,
        //     y: border_top_left_radius_height.px() as f64 - border_top_width,
        // };
        // let radii = Vec2 { x: radii.x * scale, y: radii.y * scale };
        // let angle_start = solve_start_angle_for_border(border_top_width, border_left_width, radii.x, radii.y);
        // let arc = Arc::new(left_center, radii, PI + FRAC_PI_2,  -angle_start, 0.0);
        // push_arc(&mut path, arc, tolerance);



        // draw the rightmost outer arc

        // // If the inner radii is bigger than 0, draw the inner arc
        // if inner_radii.x > 0.0 && inner_radii.y > 0.0 {
        //     for el in
        //         Arc::new(center, inner_radii, PI + FRAC_PI_2, -FRAC_PI_2, 0.0).path_elements(tolerance)
        //     {
        //         match el {
        //             PathEl::MoveTo(a) => path.push(PathEl::LineTo(a)),
        //             _ => path.push(el),
        //         }
        //     }
        // } else {
        //     path.push(PathEl::LineTo(center));
        // }

        path
    }
}

fn push_arc(path: &mut Vec<PathEl>, arc: Arc, tolerance: f64) {
    let mut elements = arc.path_elements(tolerance);
    match elements.next().unwrap() {
        PathEl::MoveTo(a) if path.len() > 0 => path.push(PathEl::LineTo(a)),
        el => path.push(el),
    }
    path.extend(elements)
}

fn solve_start_angle_for_border(bt_width: f64, br_width: f64, b_rad_x: f64, b_rad_y: f64) -> f64 {
    // slope of the border intersection split
    let w = bt_width / br_width;
    let x = b_rad_y / (w * b_rad_x);
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

    dbg!(solve_start_angle_for_border(4.0, 1.0, 1.0, 2.0));
}
