//! A rounded rect closer to the browser
//! Implemented in such a way that splits the border into 4 parts at the midway of each radius
//!
//! Can I just say, this is a lot of work for a border
//! HTML/css is annoyingly wild

use std::{f64::consts::FRAC_PI_2, f64::consts::PI};
use style::{properties::style_structs::Border, values::computed::CSSPixelLength};
use vello::kurbo::{
    Arc, ArcAppendIter, BezPath, CubicBez, Ellipse, PathEl, Point, Rect, RoundedRect,
    RoundedRectRadii, Shape, Vec2,
};

/// Resolved positions, thicknesses, and radii using the document scale and layout data
///
/// This should be calculated once and then used to stroke borders, outlines, and frames
#[derive(Debug, Clone, Copy)]
pub struct ResolvedBorderLayout {
    rect: Rect,

    border_top_width: f64,
    border_left_width: f64,
    border_right_width: f64,
    border_bottom_width: f64,

    border_top_left_radius_width: f64,
    border_top_left_radius_height: f64,
    border_top_right_radius_width: f64,
    border_top_right_radius_height: f64,

    border_bottom_left_radius_width: f64,
    border_bottom_left_radius_height: f64,
    border_bottom_right_radius_width: f64,
    border_bottom_right_radius_height: f64,
}

impl ResolvedBorderLayout {
    #[rustfmt::skip]
    pub fn new(border: &Border, rect: Rect, scale: f64) -> Self {

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

        let border_bottom_left_radius_width = scale * border.border_bottom_left_radius.0.width.0.resolve(pixel_width).px() as f64;
        let border_bottom_left_radius_height = scale * border.border_bottom_left_radius.0.height.0.resolve(pixel_height).px() as f64;

        let border_bottom_right_radius_width = scale * border.border_bottom_right_radius.0.width.0.resolve(pixel_width).px() as f64;
        let border_bottom_right_radius_height = scale * border.border_bottom_right_radius.0.height.0.resolve(pixel_height).px() as f64;

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
            border_bottom_left_radius_width,
            border_bottom_left_radius_height,
            border_bottom_right_radius_width,
            border_bottom_right_radius_height,
        }
    }

    pub fn border(&self, edge: Edge, tolerance: f64) -> BezPath {
        use ArcSide::*;
        use Corner::*;
        use Edge::*;

        let mut path = BezPath::new();

        let (c0, c1) = match edge {
            Top => (TopLeft, TopRight),
            Right => (TopRight, BottomRight),
            Bottom => (BottomRight, BottomLeft),
            Left => (BottomLeft, TopLeft),
        };

        // 1. First corner
        // if the radius is bigger than the border, we need to draw the inner arc to fill in the gap
        if self.is_sharp(c0) {
            path.move_to(self.corner(c0, Inner));
            path.line_to(self.corner(c0, Outer));
        } else {
            match self.corner_needs_infill(c0) {
                true => path.insert_arc(self.arc(c0, Inner, edge), tolerance),
                false => path.move_to(self.corner(c0, Inner)),
            }
            path.insert_arc(self.arc(c0, Outer, edge), tolerance);
        }

        // 2. Second corner
        if self.is_sharp(c1) {
            path.line_to(self.corner(c1, Outer));
            path.line_to(self.corner(c1, Inner));
        } else {
            dbg!(c0, c1, self.corner_needs_infill(c1));
            path.insert_arc(self.arc(c1, Outer, edge), tolerance);
            match self.corner_needs_infill(c1) {
                true => {
                    println!("filling arc");
                    path.insert_arc(self.arc(c1, Inner, edge), tolerance)
                }
                false => path.line_to(self.corner(c1, Inner)),
            }
        }

        path
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
            Corner::BottomRight => match side {
                ArcSide::Inner => (
                    self.rect.x1 - self.border_right_width,
                    self.rect.y1 - self.border_bottom_width,
                ),
                ArcSide::Outer => (self.rect.x1, self.rect.y1),
            },
            Corner::BottomLeft => match side {
                ArcSide::Inner => (
                    self.rect.x0 + self.border_left_width,
                    self.rect.y1 - self.border_bottom_width,
                ),
                ArcSide::Outer => (self.rect.x0, self.rect.y1),
            },
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
            border_bottom_left_radius_width,
            border_bottom_left_radius_height,
            border_bottom_right_radius_width,
            border_bottom_right_radius_height,
        } = self;

        match corner {
            Corner::TopLeft => {
                border_top_left_radius_width > border_left_width
                    && border_top_left_radius_height > border_top_width
            }
            Corner::TopRight => {
                border_top_right_radius_width > border_right_width
                    && border_top_right_radius_height > border_top_width
            }
            Corner::BottomRight => {
                border_bottom_right_radius_width > border_right_width
                    && border_bottom_right_radius_height > border_bottom_width
            }
            Corner::BottomLeft => {
                border_bottom_left_radius_width > border_left_width
                    && border_bottom_left_radius_height > border_bottom_width
            }
        }
    }

    // Get the arc for a corner
    fn arc(&self, corner: Corner, side: ArcSide, edge: Edge) -> Arc {
        use ArcSide::*;
        use Corner::*;
        use Edge::*;

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

        // Easier to reason about if we think about just offsetting the turns from the start
        let offset = match edge {
            Top => 0.0,
            Right => FRAC_PI_2,
            Bottom => PI,
            Left => PI + FRAC_PI_2,
        };

        // Depededning on the edge, we need to adjust the start angle
        // We still sweep the same, but the theta split is different since we're cutting in half
        let (start, sweep) = match (edge, corner, side) {
            // Top Edge
            (Top, TopLeft, Inner) => (0.0, FRAC_PI_2 - theta),
            (Top, TopLeft, Outer) => (theta - FRAC_PI_2, FRAC_PI_2 - theta),
            (Top, TopRight, Outer) => (0.0, FRAC_PI_2 - theta),
            (Top, TopRight, Inner) => (FRAC_PI_2 - theta, FRAC_PI_2 - theta),

            // Right Edge
            (Right, TopRight, Inner) => (0.0, theta),
            (Right, TopRight, Outer) => (-theta, theta),
            (Right, BottomRight, Outer) => (0.0, theta),
            (Right, BottomRight, Inner) => (theta, theta),

            // Bottom Edge
            (Bottom, BottomRight, Inner) => (0.0, FRAC_PI_2 - theta),
            (Bottom, BottomRight, Outer) => (theta - FRAC_PI_2, FRAC_PI_2 - theta),
            (Bottom, BottomLeft, Outer) => (0.0, FRAC_PI_2 - theta),
            (Bottom, BottomLeft, Inner) => (FRAC_PI_2 - theta, FRAC_PI_2 - theta),

            // Left Edge
            (Left, BottomLeft, Inner) => (0.0, theta),
            (Left, BottomLeft, Outer) => (-theta, theta),
            (Left, TopLeft, Outer) => (0.0, theta),
            (Left, TopLeft, Inner) => (theta, theta),

            _ => todo!(),
        };

        Arc::new(
            pair.center,
            radii,
            start + PI + FRAC_PI_2 + offset,
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
            Corner::BottomRight => {
                self.border_bottom_right_radius_width == 0.0
                    || self.border_bottom_right_radius_height == 0.0
            }
            Corner::BottomLeft => {
                self.border_bottom_left_radius_width == 0.0
                    || self.border_bottom_left_radius_height == 0.0
            }
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
            border_bottom_width,
            border_bottom_left_radius_width,
            border_bottom_left_radius_height,
            border_bottom_right_radius_width,
            border_bottom_right_radius_height,

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
            Corner::BottomRight => {
                outer = Vec2 { x: *border_bottom_right_radius_width, y: *border_bottom_right_radius_height };
                inner = Vec2 { x: border_bottom_right_radius_width - border_right_width, y: border_bottom_right_radius_height - border_bottom_width };
                center = rect.origin() + Vec2 { x: rect.width() - outer.x, y: rect.height() - outer.y } ;
            },
            Corner::BottomLeft => {
                outer = Vec2 { x: *border_bottom_left_radius_width, y: *border_bottom_left_radius_height };
                inner = Vec2 { x: border_bottom_left_radius_width - border_left_width, y: border_bottom_left_radius_height - border_bottom_width };
                center = rect.origin() + Vec2 { x: outer.x, y: rect.height() - outer.y } ;
            },
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
pub enum Edge {
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
