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

impl Document {
    // please pay a smart person to simplify this
    pub fn top_segment(&self, rect: Rect, border: &Border, tolerance: f64) -> BezPath {
        use ArcSide::*;
        use Corner::*;

        let frame = ResolvedBorderLayout::new(border, rect, self.viewport.scale_f64());
        let mut path = BezPath::new();

        // 1. Top left corner
        if frame.is_sharp(TopLeft) {
            path.move_to(frame.corner(TopLeft, Inner));
            path.line_to(frame.corner(TopLeft, Outer));
        } else {
            // if the radius is bigger than the border, we need to draw the inner arc to fill in the gap
            match frame.corner_needs_infill(TopLeft) {
                true => path.insert_arc(frame.arc(TopLeft, ArcSide::Inner, Edge::Top), tolerance),
                false => path.move_to(frame.corner(TopLeft, Inner)),
            }
            path.insert_arc(frame.arc(TopLeft, ArcSide::Outer, Edge::Top), tolerance);
        }

        // 2. Top right corner
        if frame.is_sharp(TopRight) {
            path.line_to(frame.corner(TopRight, Outer));
            path.line_to(frame.corner(TopRight, Inner));
        } else {
            let pair = frame.radii(TopRight);

            // path.insert_arc(frame.arc(TopRight, ArcSide::Outer, Edge::Top), tolerance);
            // Draw the outer arc
            let angle_start = frame.start_angle(TopRight, pair.outer);
            path.insert_arc(
                Arc::new(
                    pair.center,
                    pair.outer,
                    PI + FRAC_PI_2,
                    FRAC_PI_2 - angle_start,
                    0.0,
                ),
                tolerance,
            );

            // if the radius is bigger than the border, we need to draw the inner arc to fill in the gap
            if frame.corner_needs_infill(TopRight) {
                let angle_start = frame.start_angle(TopRight, pair.inner);
                path.insert_arc(
                    Arc::new(
                        pair.center,
                        pair.inner,
                        -angle_start,
                        -(FRAC_PI_2 - angle_start),
                        0.0,
                    ),
                    tolerance,
                );
            } else {
                path.line_to(frame.corner(TopRight, Inner));
            }
        }

        path
    }
}

/// Resolved positions, thicknesses, and radii using the document scale and layout data
#[derive(Debug, Clone, Copy)]
struct ResolvedBorderLayout {
    rect: Rect,
    border_top_width: f64,
    border_left_width: f64,
    border_right_width: f64,
    border_bottom_width: f64,
    border_top_left_radius_width: f64,
    border_top_left_radius_height: f64,
    border_top_right_radius_width: f64,
    border_top_right_radius_height: f64,
}
impl ResolvedBorderLayout {
    #[rustfmt::skip]
    fn new(border: &Border, rect: Rect, scale: f64) -> Self {

        // Resolve the radii to a length. need to downscale since the radii are in document pixels
        let pixel_width = CSSPixelLength::new((rect.width() / scale) as _);
        let pixel_height = CSSPixelLength::new((rect.height() / scale) as _);

        // Resolve and rescale
        // We have to scale since document pixels are not same same as rendered pixels
        let border_top_width = scale * border.border_top_width.to_f64_px();
        let border_left_width = scale * border.border_left_width.to_f64_px();
        let border_right_width = scale * border.border_right_width.to_f64_px();
        let border_bottom_width = scale * border.border_bottom_width.to_f64_px();
        let border_top_left_radius_width = scale * border.border_top_left_radius.0.width.0.resolve(pixel_width).px() as f64;
        let border_top_left_radius_height = scale * border.border_top_left_radius.0.height.0.resolve(pixel_height).px() as f64;
        let border_top_right_radius_width = scale * border.border_top_right_radius.0.width.0.resolve(pixel_width).px() as f64;
        let border_top_right_radius_height = scale * border.border_top_right_radius.0.height.0.resolve(pixel_height).px() as f64;

        Self {
            rect,
            border_top_width,
            border_left_width,
            border_right_width,
            border_bottom_width,
            border_top_left_radius_width,
            border_top_left_radius_height,
            border_top_right_radius_width,
            border_top_right_radius_height,
        }
    }

    fn corner(&self, corner: Corner, side: ArcSide) -> Point {
        let (x, y) = match corner {
            Corner::TopLeft => match side {
                ArcSide::Inner => (
                    self.rect.x0 + self.border_left_width,
                    self.rect.y0 + self.border_top_width,
                ),
                ArcSide::Outer => (self.rect.x0, self.rect.y0),
            },
            Corner::TopRight => match side {
                ArcSide::Inner => (
                    self.rect.x1 - self.border_right_width,
                    self.rect.y0 + self.border_top_width,
                ),
                ArcSide::Outer => (self.rect.x1, self.rect.y0),
            },
            Corner::BottomLeft => todo!(),
            Corner::BottomRight => todo!(),
        };

        Point { x, y }
    }

    /// Check if the corner width is smaller than the radius.
    /// If it is, we need to fill in the gap with an arc
    fn corner_needs_infill(&self, corner: Corner) -> bool {
        let Self {
            rect,
            border_top_width,
            border_left_width,
            border_right_width,
            border_bottom_width,
            border_top_left_radius_width,
            border_top_left_radius_height,
            border_top_right_radius_width,
            border_top_right_radius_height,
        } = self;

        match corner {
            Corner::TopLeft => {
                border_top_left_radius_width > border_left_width
                    || border_top_left_radius_height > border_top_width
            }
            Corner::TopRight => {
                border_top_right_radius_width > border_right_width
                    || border_top_right_radius_height > border_top_width
            }
            Corner::BottomLeft => todo!(),
            Corner::BottomRight => todo!(),
        }
    }

    // Get the arc for a corner
    fn arc(&self, corner: Corner, side: ArcSide, edge: Edge) -> Arc {
        let pair = self.radii(corner);
        let radii = match side {
            ArcSide::Inner => pair.inner,
            ArcSide::Outer => pair.outer,
        };
        // We solve a tiny system of equations to find the start angle
        // This is fixed to a single coordinate system, so we need to adjust the start angle
        // This is done in the matching down below
        let theta = match side {
            ArcSide::Inner => self.start_angle(corner, pair.inner),
            ArcSide::Outer => self.start_angle(corner, pair.outer),
        };

        // Sweep clockwise for outer arcs, counter clockwise for inner arcs
        let sweep_direction = match side {
            ArcSide::Inner => -1.0,
            ArcSide::Outer => 1.0,
        };

        let start;
        let sweep;

        // Depededning on the edge, we need to adjust the start angle
        // We still sweep the same, but the theta split is different since we're cutting in half
        match edge {
            Edge::Top => {}
            Edge::Right => {}
            Edge::Bottom => {}
            Edge::Left => {}
        };

        match corner {
            Corner::TopLeft => match side {
                ArcSide::Inner => {
                    start = 0.0;
                    sweep = FRAC_PI_2 - theta;
                }
                ArcSide::Outer => {
                    start = theta - FRAC_PI_2;
                    sweep = FRAC_PI_2 - theta;
                }
            },
            Corner::TopRight => match side {
                ArcSide::Outer => {
                    start = 0.0;
                    sweep = FRAC_PI_2 - theta;
                }
                ArcSide::Inner => {
                    start = -theta;
                    sweep = -(FRAC_PI_2 - theta);
                }
            },
            Corner::BottomLeft => todo!(),
            Corner::BottomRight => todo!(),
        };

        Arc::new(
            pair.center,
            radii,
            start + PI + FRAC_PI_2,
            sweep * sweep_direction,
            0.0,
        )
    }

    /// Check if a corner is sharp (IE the absolute radius is 0)
    fn is_sharp(&self, corner: Corner) -> bool {
        match corner {
            Corner::TopLeft => {
                self.border_top_left_radius_width == 0.0
                    || self.border_top_left_radius_height == 0.0
            }
            Corner::TopRight => {
                self.border_top_right_radius_width == 0.0
                    || self.border_top_right_radius_height == 0.0
            }
            Corner::BottomLeft => todo!(),
            Corner::BottomRight => todo!(),
        }
    }

    #[rustfmt::skip]
    fn radii(&self, corner: Corner) -> RadiiPair {
        let ResolvedBorderLayout {
            border_top_width,
            border_left_width,
            border_right_width,
            border_top_left_radius_width,
            border_top_left_radius_height,
            border_top_right_radius_width,
            border_top_right_radius_height,
            rect,
            ..
        } = self;

        let (outer, inner, center);

        match corner {
            Corner::TopLeft => {
                outer = Vec2 { x: *border_top_left_radius_width, y: *border_top_left_radius_height };
                inner = Vec2 { x: border_top_left_radius_width - border_left_width, y: border_top_left_radius_height - border_top_width };
                center = rect.origin() + outer;
            }
            Corner::TopRight => {
                outer = Vec2 { x: *border_top_right_radius_width, y: *border_top_right_radius_height };
                inner = Vec2 { x: border_top_right_radius_width - border_right_width, y: border_top_right_radius_height - border_top_width };
                center = rect.origin() + Vec2 { x: rect.width() - outer.x, y: outer.y } ;
            },
            Corner::BottomLeft => todo!(),
            Corner::BottomRight => todo!(),
        }

        RadiiPair { corner, inner, outer, center }
    }

    fn start_angle(&self, corner: Corner, radii: Vec2) -> f64 {
        match corner {
            Corner::TopLeft => {
                solve_start_angle_for_border(self.border_top_width, self.border_left_width, radii)
            }
            Corner::TopRight => {
                solve_start_angle_for_border(self.border_top_width, self.border_right_width, radii)
            }
            Corner::BottomLeft => solve_start_angle_for_border(
                self.border_bottom_width,
                self.border_left_width,
                radii,
            ),
            Corner::BottomRight => solve_start_angle_for_border(
                self.border_bottom_width,
                self.border_right_width,
                radii,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RadiiPair {
    inner: Vec2,
    outer: Vec2,
    center: Point,
    corner: Corner,
}

#[derive(Debug, Clone, Copy)]
enum ArcSide {
    Inner,
    Outer,
}

#[derive(Debug, Clone, Copy)]
enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Debug, Clone, Copy)]
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
