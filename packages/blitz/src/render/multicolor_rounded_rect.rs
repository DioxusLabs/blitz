//! A rounded rect closer to the browser
//! Implemented in such a way that splits the border into 4 parts at the midway of each radius
//!
//! This object is meant to be updated only when the data changes - BezPaths are expensive!
//!
//! Can I just say, this is a lot of work for a border
//! HTML/css is annoyingly wild

use std::{f64::consts::FRAC_PI_2, f64::consts::PI};
use style::{properties::ComputedValues, values::computed::CSSPixelLength};
use taffy::prelude::Layout;
use vello::kurbo::{Arc, BezPath, Ellipse, PathEl, Point, Rect, Shape, Vec2};

/// Resolved positions, thicknesses, and radii using the document scale and layout data
///
/// This should be calculated once and then used to stroke borders, outlines, and frames
///
/// It contains all the information needed to draw the frame, border, etc
///
#[derive(Debug, Clone)]
pub struct ElementFrame {
    pub outer_rect: Rect,
    pub inner_rect: Rect,
    pub outline_width: f64,

    pub border_top_width: f64,
    pub border_left_width: f64,
    pub border_right_width: f64,
    pub border_bottom_width: f64,

    pub border_top_left_radius_width: f64,
    pub border_top_left_radius_height: f64,
    pub border_top_right_radius_width: f64,
    pub border_top_right_radius_height: f64,

    pub border_bottom_left_radius_width: f64,
    pub border_bottom_left_radius_height: f64,
    pub border_bottom_right_radius_width: f64,
    pub border_bottom_right_radius_height: f64,
}

impl ElementFrame {
    #[rustfmt::skip]
    pub fn new(style: &ComputedValues, layout: &Layout, scale: f64) -> Self {
        let (border, outline) = (style.get_border(), style.get_outline());

        // let scale = 1.0;

        // Resolve and rescale
        // We have to scale since document pixels are not same same as rendered pixels
        let border_top_width = scale * border.border_top_width.to_f64_px();
        let border_left_width = scale * border.border_left_width.to_f64_px();
        let border_right_width = scale * border.border_right_width.to_f64_px();
        let border_bottom_width = scale * border.border_bottom_width.to_f64_px();
        let outline_width =  scale * outline.outline_width.to_f64_px();

        let width: f64 = layout.size.width.into();
        let height: f64 = layout.size.height.into();
        let width =  scale * width;
        let height = scale * height;

        let outer_rect = Rect::new(0.0, 0.0, width, height);
        let inner_rect = Rect::new(
            border_left_width,
            border_top_width,
            width - border_right_width,
            height - border_bottom_width,
        );

        // Resolve the radii to a length. need to downscale since the radii are in document pixels
        let pixel_width = CSSPixelLength::new((inner_rect.width() / scale) as _);
        let pixel_height = CSSPixelLength::new((inner_rect.height() / scale) as _);

        let mut border_top_left_radius_width = scale * border.border_top_left_radius.0.width.0.resolve(pixel_width).px() as f64;
        let mut border_top_left_radius_height = scale * border.border_top_left_radius.0.height.0.resolve(pixel_height).px() as f64;

        let mut border_top_right_radius_width = scale * border.border_top_right_radius.0.width.0.resolve(pixel_width).px() as f64;
        let mut border_top_right_radius_height = scale * border.border_top_right_radius.0.height.0.resolve(pixel_height).px() as f64;

        let mut border_bottom_left_radius_width = scale * border.border_bottom_left_radius.0.width.0.resolve(pixel_width).px() as f64;
        let mut border_bottom_left_radius_height = scale * border.border_bottom_left_radius.0.height.0.resolve(pixel_height).px() as f64;

        let mut border_bottom_right_radius_width = scale * border.border_bottom_right_radius.0.width.0.resolve(pixel_width).px() as f64;
        let mut border_bottom_right_radius_height = scale * border.border_bottom_right_radius.0.height.0.resolve(pixel_height).px() as f64;

        rescale_borderers(&mut border_top_left_radius_width, &mut border_top_right_radius_width, inner_rect.width());
        rescale_borderers(&mut border_bottom_left_radius_width, &mut border_bottom_right_radius_width, inner_rect.width());
        rescale_borderers(&mut border_top_left_radius_height, &mut border_bottom_left_radius_height, inner_rect.height());
        rescale_borderers(&mut border_top_right_radius_height, &mut border_bottom_right_radius_height, inner_rect.height());

        // Correct the border radii if they are too big
        // if two border radii would intersect, we need to shrink them to the intersection point
        fn rescale_borderers(left: &mut f64, right: &mut f64, dim: f64)  {
            if *left + *right > dim {
                let ratio = *left / (*left + *right);
                *left = ratio * dim;
                *right = (1.0 - ratio) * dim;
            }
        }

        Self {
            inner_rect,
            outer_rect,
            outline_width,
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

    /// Construct a BezPath representing the edges of a border.
    ///
    /// Will construct the border by:
    /// - drawing an inner arc
    /// - jumping to an outer arc
    /// - jumping to the next outer arc (completing the edge with the previous)
    /// - drawing an inner arc
    pub fn border(&self, edge: Edge) -> BezPath {
        use {ArcSide::*, Corner::*, Direction::*, Edge::*};

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
                true => path.insert_arc(self.arc(c0, Inner, edge, Anticlockwise)),
                false => path.move_to(self.corner(c0, Inner)),
            }
            path.insert_arc(self.arc(c0, Outer, edge, Clockwise));
        }

        // 2. Second corner
        if self.is_sharp(c1) {
            path.line_to(self.corner(c1, Outer));
            path.line_to(self.corner(c1, Inner));
        } else {
            path.insert_arc(self.arc(c1, Outer, edge, Clockwise));
            match self.corner_needs_infill(c1) {
                true => path.insert_arc(self.arc(c1, Inner, edge, Anticlockwise)),
                false => path.line_to(self.corner(c1, Inner)),
            }
        }

        path
    }

    /// Construct a bezpath drawing the outline
    pub fn outline(&self) -> BezPath {
        use Corner::*;

        let mut path = BezPath::new();

        // todo: this has been known to produce quirky outputs with hugely rounded edges
        self.shape(&mut path, ArcSide::Outline, Direction::Clockwise);
        path.move_to(self.corner(TopLeft, ArcSide::Outer));

        self.shape(&mut path, ArcSide::Outer, Direction::Anticlockwise);
        path.move_to(self.corner(TopLeft, ArcSide::Outer));

        path
    }

    /// Construct a bezpath drawing the frame
    pub fn frame(&self) -> BezPath {
        let mut path = BezPath::new();
        self.shape(&mut path, ArcSide::Inner, Direction::Clockwise);
        path
    }

    fn shape(&self, path: &mut BezPath, line: ArcSide, direction: Direction) {
        use Corner::*;

        let route = match direction {
            Direction::Clockwise => [TopLeft, TopRight, BottomRight, BottomLeft],
            Direction::Anticlockwise => [TopLeft, BottomLeft, BottomRight, TopRight],
        };

        for corner in route {
            if self.is_sharp(corner) {
                path.insert_point(self.corner(corner, line));
            } else {
                path.insert_arc(self.full_arc(corner, line, direction));
            }
        }
    }

    fn corner(&self, corner: Corner, side: ArcSide) -> Point {
        let Rect { x0, y0, x1, y1 } = self.outer_rect;

        let (x, y) = match corner {
            Corner::TopLeft => match side {
                ArcSide::Inner => (x0 + self.border_left_width, y0 + self.border_top_width),
                ArcSide::Outer => (x0, y0),
                ArcSide::Outline => (x0 - self.outline_width, y0 - self.outline_width),
                ArcSide::Middle => unimplemented!(),
            },
            Corner::TopRight => match side {
                ArcSide::Inner => (x1 - self.border_right_width, y0 + self.border_top_width),
                ArcSide::Outer => (x1, y0),
                ArcSide::Outline => (x1 + self.outline_width, y0 - self.outline_width),
                ArcSide::Middle => unimplemented!(),
            },
            Corner::BottomRight => match side {
                ArcSide::Inner => (x1 - self.border_right_width, y1 - self.border_bottom_width),
                ArcSide::Outer => (x1, y1),
                ArcSide::Outline => (x1 + self.outline_width, y1 + self.outline_width),
                ArcSide::Middle => unimplemented!(),
            },
            Corner::BottomLeft => match side {
                ArcSide::Inner => (x0 + self.border_left_width, y1 - self.border_bottom_width),
                ArcSide::Outer => (x0, y1),
                ArcSide::Outline => (x0 - self.outline_width, y1 + self.outline_width),
                ArcSide::Middle => unimplemented!(),
            },
        };

        Point { x, y }
    }

    /// Check if the corner width is smaller than the radius.
    /// If it is, we need to fill in the gap with an arc
    fn corner_needs_infill(&self, corner: Corner) -> bool {
        let Self {
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
            ..
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

    /// Get the complete arc for a corner, skipping the need for splitting the arc into pieces
    fn full_arc(&self, corner: Corner, side: ArcSide, direction: Direction) -> Arc {
        let ellipse = self.ellipse(corner, side);

        // Sweep clockwise for outer arcs, counter clockwise for inner arcs
        let sweep_direction = match direction {
            Direction::Anticlockwise => -1.0,
            Direction::Clockwise => 1.0,
        };

        let offset = match corner {
            Corner::TopLeft => -FRAC_PI_2,
            Corner::TopRight => 0.0,
            Corner::BottomRight => FRAC_PI_2,
            Corner::BottomLeft => PI,
        };

        let offset = match direction {
            Direction::Clockwise => offset,
            Direction::Anticlockwise => offset + FRAC_PI_2,
        };

        Arc::new(
            ellipse.center(),
            ellipse.radii(),
            // Note that we apply a fixed offset to get us in the unit circle coordinate system
            // vello chooses the x axis as the start of the arc, so we need to offset by 3pi/2
            offset + PI + FRAC_PI_2,
            FRAC_PI_2 * sweep_direction,
            0.0,
        )
    }

    /// Get the partial arc for a corner
    ///
    fn arc(&self, corner: Corner, side: ArcSide, edge: Edge, direction: Direction) -> Arc {
        use ArcSide::*;
        use Corner::*;
        use Edge::*;

        let ellipse = self.ellipse(corner, side);

        // We solve a tiny system of equations to find the start angle
        // This is fixed to a single coordinate system, so we need to adjust the start angle
        let theta = self.start_angle(corner, ellipse.radii());

        // Sweep clockwise for outer arcs, counter clockwise for inner arcs
        let sweep_direction = match direction {
            Direction::Anticlockwise => -1.0,
            Direction::Clockwise => 1.0,
        };

        // Easier to reason about if we think about just offsetting the turns from the start
        let offset = match edge {
            Top => 0.0,
            Right => FRAC_PI_2,
            Bottom => PI,
            Left => PI + FRAC_PI_2,
        };

        // On left/right gets theta, on top/bottom gets pi/2 - theta
        let theta = match edge {
            Top | Bottom => FRAC_PI_2 - theta,
            Right | Left => theta,
        };

        // Depededning on the edge, we need to adjust the start angle
        // We still sweep the same, but the theta split is different since we're cutting in half
        // I imagine you could mnake this simpler using a bit more math
        let start = match (edge, corner, side) {
            // Top Edge
            (Top, TopLeft, Inner) => 0.0,
            (Top, TopLeft, Outer) => -theta,
            (Top, TopRight, Outer) => 0.0,
            (Top, TopRight, Inner) => theta,

            // Right Edge
            (Right, TopRight, Inner) => 0.0,
            (Right, TopRight, Outer) => -theta,
            (Right, BottomRight, Outer) => 0.0,
            (Right, BottomRight, Inner) => theta,

            // Bottom Edge
            (Bottom, BottomRight, Inner) => 0.0,
            (Bottom, BottomRight, Outer) => -theta,
            (Bottom, BottomLeft, Outer) => 0.0,
            (Bottom, BottomLeft, Inner) => theta,

            // Left Edge
            (Left, BottomLeft, Inner) => 0.0,
            (Left, BottomLeft, Outer) => -theta,
            (Left, TopLeft, Outer) => 0.0,
            (Left, TopLeft, Inner) => theta,

            _ => unreachable!("Invalid edge/corner combination"),
        };

        Arc::new(
            ellipse.center(),
            ellipse.radii(),
            // Note that we apply a fixed offset to get us in the unit circle coordinate system
            // vello chooses the x axis as the start of the arc, so we need to offset by 3pi/2
            start + offset + PI + FRAC_PI_2,
            theta * sweep_direction,
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
    fn ellipse(&self, corner: Corner, side: ArcSide) -> Ellipse {
        use {Corner::*, ArcSide::*};
        let ElementFrame {
            outer_rect: rect,
            border_top_width,
            border_left_width,
            border_right_width,
            border_top_left_radius_width,
            border_top_left_radius_height,
            border_top_right_radius_width,
            border_top_right_radius_height,
            border_bottom_width,
            border_bottom_left_radius_width,
            border_bottom_left_radius_height,
            border_bottom_right_radius_width,
            border_bottom_right_radius_height,
            outline_width,
            ..
        } = self;

        let outer = match corner {
            TopLeft => (*border_top_left_radius_width, *border_top_left_radius_height),
            TopRight => (*border_top_right_radius_width, *border_top_right_radius_height),
            BottomLeft => (*border_bottom_left_radius_width, *border_bottom_left_radius_height),
            BottomRight => (*border_bottom_right_radius_width, *border_bottom_right_radius_height),
        };

        let center = match corner {
            TopLeft => outer,
            TopRight => (rect.width() - outer.0, outer.1 ).into(),
            BottomLeft => (outer.0, rect.height() - outer.1 ).into(),
            BottomRight => (rect.width() - outer.0, rect.height() - outer.1).into(),
        };

        let radii = match side {
            Outer => outer,
            Outline => match corner {
                TopLeft => (border_top_left_radius_width + outline_width, border_top_left_radius_height + outline_width),
                TopRight => (border_top_right_radius_width + outline_width, border_top_right_radius_height + outline_width),
                BottomRight => (border_bottom_right_radius_width + outline_width, border_bottom_right_radius_height + outline_width),
                BottomLeft => (border_bottom_left_radius_width + outline_width, border_bottom_left_radius_height + outline_width),
            },
            Middle => todo!("Implement border midline offset"),
            Inner => match corner {
                TopLeft => (border_top_left_radius_width - border_left_width, border_top_left_radius_height - border_top_width),
                TopRight => (border_top_right_radius_width - border_right_width, border_top_right_radius_height - border_top_width),
                BottomRight => (border_bottom_right_radius_width - border_right_width, border_bottom_right_radius_height - border_bottom_width),
                BottomLeft => (border_bottom_left_radius_width - border_left_width, border_bottom_left_radius_height - border_bottom_width),
            },
        };

        Ellipse::new(rect.origin() + center, radii, 0.0)
    }

    fn start_angle(&self, corner: Corner, radii: Vec2) -> f64 {
        use Corner::*;
        match corner {
            TopLeft => start_angle(self.border_top_width, self.border_left_width, radii),
            TopRight => start_angle(self.border_top_width, self.border_right_width, radii),
            BottomLeft => start_angle(self.border_bottom_width, self.border_left_width, radii),
            BottomRight => start_angle(self.border_bottom_width, self.border_right_width, radii),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

/// There are several corners at play here
///
/// - Outline
/// - Border
/// - Frame
///
/// So we have 4 corner distances, clockwise/anticlockwise, and 4 corners
///
/// We combine the corners and their distances to have twelve options here.
///
/// ```a
///    *-----------------------------------------------------------------*  <--- ArcSide::Outline
///    |                               Outline                           |
///    |    *--------------------------------------------------------*   |  <--- ArcSide::Outer
///    |    |                         Border Outer                   |   |
///    |    |    *----------------------------------------------*    |   |  <--- ArcSide::Middle
///    |    |    |                    Border Inner              |    |   |
///    |    |    |    *------------------------------------*    |    |   |  <--- ArcSide::Inner
///    |    |    |    |                                    |    |    |   |
///    |    |    |    |                                    |    |    |   |
///    |    |    |    |                                    |    |    |   |
///    |    |    |    |                                    |    |    |   |
///    |    |    |    |                                    |    |    |   |
///    |    |    |    |                                    |    |    |   |
///    |    |    |    |                                    |    |    |   |
///    |    |    |    |                                    |    |    |   |
///    |    |    |    *------------------------------------*    |    |   |
///    |    |    |                                              |    |   |
///    |    |    *----------------------------------------------*    |   |
///    |    |                                                        |   |
///    |    *--------------------------------------------------------*   |
///    |                                                                 |
///    *-----------------------------------------------------------------*
/// ```
#[derive(Debug, Clone, Copy)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy)]
enum ArcSide {
    Outline,
    Outer,
    Middle,
    Inner,
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Clockwise,
    Anticlockwise,
}

/// Makes it easier to insert objects into a bezpath without having to do checks
/// Mostly because I consider the vello api slightly defficient
trait BuildBezpath {
    const TOLERANCE: f64;
    fn insert_arc(&mut self, arc: Arc);
    fn insert_point(&mut self, point: Point);
}

impl BuildBezpath for BezPath {
    /// Vello uses an inner tolerance for creating segments
    /// We're just reusing the value here
    const TOLERANCE: f64 = 0.1;

    fn insert_arc(&mut self, arc: Arc) {
        let mut elements = arc.path_elements(Self::TOLERANCE);
        match elements.next().unwrap() {
            PathEl::MoveTo(a) if self.elements().len() > 0 => self.push(PathEl::LineTo(a)),
            el => self.push(el),
        }
        self.extend(elements)
    }

    fn insert_point(&mut self, point: Point) {
        if self.elements().is_empty() {
            self.push(PathEl::MoveTo(point));
        } else {
            self.push(PathEl::LineTo(point));
        }
    }
}

/// Get the start angle of the arc based on the border width and the radii
fn start_angle(bt_width: f64, br_width: f64, radii: Vec2) -> f64 {
    // slope of the border intersection split
    let w = bt_width / br_width;
    let x = radii.y / (w * radii.x);

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
    dbg!(start_angle(4.0, 1.0, Vec2 { x: 1.0, y: 2.0 }));
}
