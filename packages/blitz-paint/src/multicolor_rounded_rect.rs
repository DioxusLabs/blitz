//! A rounded rect closer to the browser
//! Implemented in such a way that splits the border into 4 parts at the midway of each radius
//!
//! This object is meant to be updated only when the data changes - BezPaths are expensive!
//!
//! Can I just say, this is a lot of work for a border
//! HTML/css is annoyingly wild

use kurbo::{Arc, BezPath, Ellipse, Insets, PathEl, Point, Rect, Shape, Vec2};
use std::{f64::consts::FRAC_PI_2, f64::consts::PI};
use style::{
    properties::ComputedValues,
    values::computed::{BorderCornerRadius, CSSPixelLength},
};
use taffy::prelude::Layout;

use crate::non_uniform_rounded_rect::NonUniformRoundedRectRadii;

fn insets_from_taffy_rect(input: taffy::Rect<f64>) -> Insets {
    Insets {
        x0: input.left,
        y0: input.top,
        x1: input.right,
        y1: input.bottom,
    }
}

fn add_insets(a: Insets, b: Insets) -> Insets {
    Insets {
        x0: a.x0 + b.x0,
        y0: a.y0 + b.y0,
        x1: a.x1 + b.x1,
        y1: a.y1 + b.y1,
    }
}

#[inline(always)]
fn get_corner_insets(insets: Insets, corner: Corner) -> Vec2 {
    match corner {
        Corner::TopLeft => Vec2 {
            x: insets.x0,
            y: insets.y0,
        },
        Corner::TopRight => Vec2 {
            x: insets.x1,
            y: insets.y0,
        },
        Corner::BottomLeft => Vec2 {
            x: insets.x0,
            y: insets.y1,
        },
        Corner::BottomRight => Vec2 {
            x: insets.x1,
            y: insets.y1,
        },
    }
}

/// Resolved positions, thicknesses, and radii using the document scale and layout data
///
/// This should be calculated once and then used to stroke borders, outlines, and frames
///
/// It contains all the information needed to draw the frame, border, etc
///
#[derive(Debug, Clone)]
pub struct CssRect {
    pub border_box: Rect,
    pub padding_box: Rect,
    pub content_box: Rect,
    pub outline_box: Rect,

    pub outline_width: f64,

    pub padding_width: Insets,
    pub border_width: Insets,

    pub border_radii: NonUniformRoundedRectRadii,
}

impl CssRect {
    pub fn new(style: &ComputedValues, layout: &Layout, scale: f64) -> Self {
        let s_border = style.get_border();
        let outline = style.get_outline();

        // Resolve and rescale
        // We have to scale since document pixels are not same same as rendered pixels
        let width: f64 = layout.size.width as f64 * scale;
        let height: f64 = layout.size.height as f64 * scale;
        let border = insets_from_taffy_rect(layout.border.map(|p| p as f64 * scale));
        let padding = insets_from_taffy_rect(layout.padding.map(|p| p as f64 * scale));
        let outline_width = scale * outline.outline_width.to_f64_px();

        let border_box = Rect::new(0.0, 0.0, width, height);
        let padding_box = border_box - border;
        let content_box = padding_box - padding;
        let outline_box = border_box.inset(outline_width);

        // Resolve the radii to a length. need to downscale since the radii are in document pixels
        let resolve_w = CSSPixelLength::new((padding_box.width() / scale) as _);
        let resolve_h = CSSPixelLength::new((padding_box.height() / scale) as _);
        let resolve_radii = |radius: &BorderCornerRadius| -> Vec2 {
            Vec2 {
                x: scale * radius.0.width.0.resolve(resolve_w).px() as f64,
                y: scale * radius.0.height.0.resolve(resolve_h).px() as f64,
            }
        };
        let mut border_radii = NonUniformRoundedRectRadii {
            top_left: resolve_radii(&s_border.border_top_left_radius),
            top_right: resolve_radii(&s_border.border_top_right_radius),
            bottom_right: resolve_radii(&s_border.border_bottom_right_radius),
            bottom_left: resolve_radii(&s_border.border_bottom_left_radius),
        };

        // Correct the border radii if they are too big if two border radii would intersect, then we need to shrink
        // ALL border radii by the same factor such that they do not
        let top_overlap_factor =
            border_box.width() / (border_radii.top_left.x + border_radii.top_right.x);
        let bottom_overlap_factor =
            border_box.width() / (border_radii.bottom_left.x + border_radii.bottom_right.x);
        let left_overlap_factor =
            border_box.height() / (border_radii.top_left.y + border_radii.bottom_left.y);
        let right_overlap_factor =
            border_box.height() / (border_radii.top_right.y + border_radii.bottom_right.y);

        let min_factor = top_overlap_factor
            .min(bottom_overlap_factor)
            .min(left_overlap_factor)
            .min(right_overlap_factor)
            .min(1.0);
        if min_factor < 1.0 {
            border_radii *= min_factor
        }

        Self {
            padding_box,
            border_box,
            content_box,
            outline_box,
            outline_width,
            padding_width: padding,
            border_width: border,
            border_radii,
        }
    }

    /// Construct a BezPath representing one edge of a box's border.
    /// Takes into account border-radius and the possibility that the edges
    /// are different colors.
    ///
    /// Will construct the border by:
    /// - drawing an inner arc
    /// - jumping to an outer arc
    /// - jumping to the next outer arc (completing the edge with the previous)
    /// - drawing an inner arc
    pub fn border_edge_shape(&self, edge: Edge) -> BezPath {
        use {Corner::*, CssBox::*, Direction::*, Edge::*};

        let mut path = BezPath::new();

        let (c0, c1) = match edge {
            Top => (TopLeft, TopRight),
            Right => (TopRight, BottomRight),
            Bottom => (BottomRight, BottomLeft),
            Left => (BottomLeft, TopLeft),
        };

        // 1. First corner
        // if the radius is bigger than the border, we need to draw the inner arc to fill in the gap
        if self.is_sharp(c0, BorderBox) {
            path.move_to(self.corner(c0, PaddingBox));
            path.line_to(self.corner(c0, BorderBox));
        } else {
            match self.corner_needs_infill(c0) {
                true => {
                    path.insert_arc(self.partial_corner_arc(c0, PaddingBox, edge, Anticlockwise))
                }
                false => path.move_to(self.corner(c0, PaddingBox)),
            }
            path.insert_arc(self.partial_corner_arc(c0, BorderBox, edge, Clockwise));
        }

        // 2. Second corner
        if self.is_sharp(c1, BorderBox) {
            path.line_to(self.corner(c1, BorderBox));
            path.line_to(self.corner(c1, PaddingBox));
        } else {
            path.insert_arc(self.partial_corner_arc(c1, BorderBox, edge, Clockwise));
            match self.corner_needs_infill(c1) {
                true => {
                    path.insert_arc(self.partial_corner_arc(c1, PaddingBox, edge, Anticlockwise))
                }
                false => path.line_to(self.corner(c1, PaddingBox)),
            }
        }

        path
    }

    /// Construct a bezpath drawing the outline
    pub fn outline(&self) -> BezPath {
        let mut path = BezPath::new();

        // TODO: this has been known to produce quirky outputs with hugely rounded edges
        self.shape(&mut path, CssBox::OutlineBox, Direction::Clockwise);
        path.move_to(self.corner(Corner::TopLeft, CssBox::BorderBox));

        self.shape(&mut path, CssBox::BorderBox, Direction::Anticlockwise);
        path.move_to(self.corner(Corner::TopLeft, CssBox::BorderBox));

        path
    }

    /// Construct a bezpath drawing the frame border
    pub fn border_box_path(&self) -> BezPath {
        let mut path = BezPath::new();
        self.shape(&mut path, CssBox::BorderBox, Direction::Clockwise);
        path
    }

    /// Construct a bezpath drawing the frame padding
    pub fn padding_box_path(&self) -> BezPath {
        let mut path = BezPath::new();
        self.shape(&mut path, CssBox::PaddingBox, Direction::Clockwise);
        path
    }

    /// Construct a bezpath drawing the frame content
    pub fn content_box_path(&self) -> BezPath {
        let mut path = BezPath::new();
        self.shape(&mut path, CssBox::ContentBox, Direction::Clockwise);
        path
    }

    fn shape(&self, path: &mut BezPath, line: CssBox, direction: Direction) {
        use Corner::*;

        let route = match direction {
            Direction::Clockwise => [TopLeft, TopRight, BottomRight, BottomLeft],
            Direction::Anticlockwise => [TopLeft, BottomLeft, BottomRight, TopRight],
        };

        for corner in route {
            if self.is_sharp(corner, line) {
                path.insert_point(self.corner(corner, line));
            } else {
                path.insert_arc(self.corner_arc(corner, line, direction));
            }
        }
    }

    /// Construct a bezpath drawing the frame
    pub fn shadow_clip(&self, shadow_rect: Rect) -> BezPath {
        let mut path = BezPath::new();
        self.shadow_clip_shape(&mut path, shadow_rect);
        path
    }

    fn shadow_clip_shape(&self, path: &mut BezPath, shadow_rect: Rect) {
        use Corner::*;

        for corner in [TopLeft, TopRight, BottomRight, BottomLeft] {
            path.insert_point(self.shadow_clip_corner(corner, shadow_rect));
        }

        if self.is_sharp(TopLeft, CssBox::BorderBox) {
            path.move_to(self.corner(TopLeft, CssBox::BorderBox));
        } else {
            const TOLERANCE: f64 = 0.1;
            let arc = self.corner_arc(TopLeft, CssBox::BorderBox, Direction::Anticlockwise);
            let elements = arc.path_elements(TOLERANCE);
            path.extend(elements);
        }

        for corner in [/*TopLeft, */ BottomLeft, BottomRight, TopRight] {
            if self.is_sharp(corner, CssBox::BorderBox) {
                path.insert_point(self.corner(corner, CssBox::BorderBox));
            } else {
                path.insert_arc(self.corner_arc(
                    corner,
                    CssBox::BorderBox,
                    Direction::Anticlockwise,
                ));
            }
        }
    }

    fn corner(&self, corner: Corner, css_box: CssBox) -> Point {
        let Rect { x0, y0, x1, y1 } = match css_box {
            CssBox::OutlineBox => self.outline_box,
            CssBox::BorderBox => self.border_box,
            CssBox::PaddingBox => self.padding_box,
            CssBox::ContentBox => self.content_box,
        };
        match corner {
            Corner::TopLeft => Point { x: x0, y: y0 },
            Corner::TopRight => Point { x: x1, y: y0 },
            Corner::BottomLeft => Point { x: x0, y: y1 },
            Corner::BottomRight => Point { x: x1, y: y1 },
        }
    }

    fn shadow_clip_corner(&self, corner: Corner, shadow_rect: Rect) -> Point {
        let (x, y) = match corner {
            Corner::TopLeft => (shadow_rect.x0, shadow_rect.y0),
            Corner::TopRight => (shadow_rect.x1, shadow_rect.y0),
            Corner::BottomRight => (shadow_rect.x1, shadow_rect.y1),
            Corner::BottomLeft => (shadow_rect.x0, shadow_rect.y1),
        };

        Point { x, y }
    }

    /// Check if the corner width is smaller than the radius.
    /// If it is, we need to fill in the gap with an arc
    fn corner_needs_infill(&self, corner: Corner) -> bool {
        match corner {
            Corner::TopLeft => {
                self.border_radii.top_left.x > self.border_width.x0
                    && self.border_radii.top_left.y > self.border_width.y0
            }
            Corner::TopRight => {
                self.border_radii.top_right.x > self.border_width.x1
                    && self.border_radii.top_right.y > self.border_width.y0
            }
            Corner::BottomRight => {
                self.border_radii.bottom_right.x > self.border_width.x1
                    && self.border_radii.bottom_right.y > self.border_width.y1
            }
            Corner::BottomLeft => {
                self.border_radii.bottom_left.x > self.border_width.x0
                    && self.border_radii.bottom_left.y > self.border_width.y1
            }
        }
    }

    /// Get the complete arc for a corner, skipping the need for splitting the arc into pieces
    fn corner_arc(&self, corner: Corner, css_box: CssBox, direction: Direction) -> Arc {
        let ellipse = self.ellipse(corner, css_box);

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

    /// Get the arc for a half of a corner.
    /// This handles the case where adjacent border sides have different colors and thus
    /// the corner between the side changes color in the middle. We draw these as separate shapes
    /// and thus need to to get the arc up to the "middle point".
    ///
    /// The angle at which the color changes depends on the ratio of the border widths and radii of the corner
    fn partial_corner_arc(
        &self,
        corner: Corner,
        css_box: CssBox,
        edge: Edge,
        direction: Direction,
    ) -> Arc {
        use Corner::*;
        use CssBox::*;
        use Edge::*;

        let ellipse = self.ellipse(corner, css_box);

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
        let start = match (edge, corner, css_box) {
            // Top Edge
            (Top, TopLeft, PaddingBox) => 0.0,
            (Top, TopLeft, BorderBox) => -theta,
            (Top, TopRight, BorderBox) => 0.0,
            (Top, TopRight, PaddingBox) => theta,

            // Right Edge
            (Right, TopRight, PaddingBox) => 0.0,
            (Right, TopRight, BorderBox) => -theta,
            (Right, BottomRight, BorderBox) => 0.0,
            (Right, BottomRight, PaddingBox) => theta,

            // Bottom Edge
            (Bottom, BottomRight, PaddingBox) => 0.0,
            (Bottom, BottomRight, BorderBox) => -theta,
            (Bottom, BottomLeft, BorderBox) => 0.0,
            (Bottom, BottomLeft, PaddingBox) => theta,

            // Left Edge
            (Left, BottomLeft, PaddingBox) => 0.0,
            (Left, BottomLeft, BorderBox) => -theta,
            (Left, TopLeft, BorderBox) => 0.0,
            (Left, TopLeft, PaddingBox) => theta,

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
    fn is_sharp(&self, corner: Corner, side: CssBox) -> bool {
        use Corner::*;
        use CssBox::*;

        let corner_radii = match corner {
            TopLeft => self.border_radii.top_left,
            TopRight => self.border_radii.top_right,
            BottomLeft => self.border_radii.bottom_left,
            BottomRight => self.border_radii.bottom_right,
        };
        let is_sharp = (corner_radii.x == 0.0) | (corner_radii.y == 0.0);
        if is_sharp {
            return true;
        }

        let css_box: Insets = match side {
            OutlineBox => return false,
            BorderBox => return false,
            PaddingBox => self.border_width,
            ContentBox => add_insets(self.border_width, self.padding_width),
        };
        match corner {
            TopLeft => (corner_radii.x <= css_box.x0) | (corner_radii.y <= css_box.y0),
            TopRight => (corner_radii.x <= css_box.x1) | (corner_radii.y <= css_box.y0),
            BottomLeft => (corner_radii.x <= css_box.x0) | (corner_radii.y <= css_box.y1),
            BottomRight => (corner_radii.x <= css_box.x1) | (corner_radii.y <= css_box.y1),
        }
    }

    fn ellipse(&self, corner: Corner, side: CssBox) -> Ellipse {
        use {Corner::*, CssBox::*};
        let CssRect {
            border_box,
            padding_width,
            border_width,
            border_radii,
            ..
        } = self;

        let corner_radii = match corner {
            TopLeft => border_radii.top_left,
            TopRight => border_radii.top_right,
            BottomLeft => border_radii.bottom_left,
            BottomRight => border_radii.bottom_right,
        };

        let center = match corner {
            TopLeft => corner_radii,
            TopRight => Vec2 {
                x: border_box.width() - corner_radii.x,
                y: corner_radii.y,
            },
            BottomLeft => Vec2 {
                x: corner_radii.x,
                y: border_box.height() - corner_radii.y,
            },
            BottomRight => Vec2 {
                x: border_box.width() - corner_radii.x,
                y: border_box.height() - corner_radii.y,
            },
        };

        let radii: Vec2 = match side {
            BorderBox => corner_radii,
            OutlineBox => corner_radii + Vec2::new(self.outline_width, self.outline_width),
            PaddingBox => corner_radii - get_corner_insets(*border_width, corner),
            ContentBox => {
                corner_radii - get_corner_insets(add_insets(*border_width, *padding_width), corner)
            }
        };

        Ellipse::new(border_box.origin() + center, radii, 0.0)
    }

    fn start_angle(&self, corner: Corner, radii: Vec2) -> f64 {
        let corner_insets = get_corner_insets(self.border_width, corner);
        start_angle(corner_insets.y, corner_insets.x, radii)
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
///    *--------------------------------------------------------*  <--- ArcSide::Outline
///    |                         Outline                        |
///    |    *----------------------------------------------*    |  <--- ArcSide::Outer
///    |    |                    Border                    |    |
///    |    |    *------------------------------------*    |    |  <--- ArcSide::Inner
///    |    |    |               Padding              |    |    |
///    |    |    |    *--------------------------*    |    |    |  <--- ArcSide::Content
///    |    |    |    |                          |    |    |    |
///    |    |    |    |                          |    |    |    |
///    |    |    |    |                          |    |    |    |
///    |    |    |    |                          |    |    |    |
///    |    |    |    *--------------------------*    |    |    |
///    |    |    |                                    |    |    |
///    |    |    *------------------------------------*    |    |
///    |    |                                              |    |
///    |    *----------------------------------------------*    |
///    |                                                        |
///    *--------------------------------------------------------*
/// ```
#[derive(Debug, Clone, Copy)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::enum_variant_names, reason = "Use CSS standard terminology")]
enum CssBox {
    OutlineBox,
    BorderBox,
    PaddingBox,
    ContentBox,
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
            PathEl::MoveTo(a) if !self.elements().is_empty() => self.push(PathEl::LineTo(a)),
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
