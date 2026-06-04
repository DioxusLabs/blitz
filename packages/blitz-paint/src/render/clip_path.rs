use super::ElementCx;
use kurbo::{BezPath, Circle, Ellipse, Point, Rect, Shape, SvgArc, Vec2};
use style::values::computed::basic_shape::{BasicShape, ClipPath};
use style::values::computed::{Angle, CSSPixelLength, LengthPercentage};
use style::values::generics::basic_shape::{
    ArcSize, ArcSweep, AxisEndPoint, AxisPosition, CommandEndPoint, ControlPoint, ControlReference,
    GenericBasicShape, GenericPathOrShapeFunction, GenericShapeCommand, GenericShapeRadius,
    ShapeBox, ShapeGeometryBox,
};
use style::values::generics::position::{GenericPosition, GenericPositionOrAuto};

impl ElementCx<'_, '_> {
    /// Compute the clip-path BezPath (if any) for this element.
    /// Returns `None` if clip-path is `none` or unsupported.
    pub(super) fn clip_path_shape(&self) -> Option<BezPath> {
        let clip_path = self.style.clone_clip_path();
        match clip_path {
            ClipPath::None => None,
            ClipPath::Url(_) => {
                // URL references (e.g. `clip-path: url(#myClip)`) are not yet supported
                None
            }
            ClipPath::Shape(basic_shape, geometry_box) => {
                let reference_box = self.resolve_geometry_box(&geometry_box);
                self.basic_shape_to_path(&basic_shape, reference_box)
            }
            ClipPath::Box(geometry_box) => {
                let reference_box = self.resolve_geometry_box(&geometry_box);
                Some(Rect::from(reference_box).into_path(0.1))
            }
        }
    }

    /// Resolve a ShapeGeometryBox to a concrete rectangle (x, y, width, height) in scaled pixels
    ///
    /// For SVG elements without associated CSS layout box, the used value for content-box and padding-box is fill-box and for border-box and margin-box is stroke-box.
    /// For elements with associated CSS layout box, the used value for fill-box is content-box and for stroke-box and view-box is border-box.
    fn resolve_geometry_box(&self, geometry_box: &ShapeGeometryBox) -> ReferenceBox {
        match geometry_box {
            ShapeGeometryBox::ElementDependent
            | ShapeGeometryBox::StrokeBox
            | ShapeGeometryBox::ViewBox
            | ShapeGeometryBox::ShapeBox(ShapeBox::BorderBox) => ReferenceBox {
                x: 0.0,
                y: 0.0,
                width: self.frame.border_box.width() / self.scale,
                height: self.frame.border_box.height() / self.scale,
            },
            ShapeGeometryBox::ShapeBox(ShapeBox::PaddingBox) => ReferenceBox {
                x: self.frame.border_width.x0 / self.scale,
                y: self.frame.border_width.y0 / self.scale,
                width: self.frame.padding_box.width() / self.scale,
                height: self.frame.padding_box.height() / self.scale,
            },
            ShapeGeometryBox::FillBox | ShapeGeometryBox::ShapeBox(ShapeBox::ContentBox) => {
                ReferenceBox {
                    x: (self.frame.border_width.x0 + self.frame.padding_width.x0) / self.scale,
                    y: (self.frame.border_width.y0 + self.frame.padding_width.y0) / self.scale,
                    width: self.frame.content_box.width() / self.scale,
                    height: self.frame.content_box.height() / self.scale,
                }
            }
            ShapeGeometryBox::ShapeBox(ShapeBox::MarginBox) => {
                // Margin box is not tracked in CssBox, fall back to border box
                ReferenceBox {
                    x: 0.0,
                    y: 0.0,
                    width: self.frame.border_box.width() / self.scale,
                    height: self.frame.border_box.height() / self.scale,
                }
            }
        }
    }

    /// Convert a CSS basic-shape to a kurbo BezPath
    fn basic_shape_to_path(
        &self,
        shape: &BasicShape,
        reference_box: ReferenceBox,
    ) -> Option<BezPath> {
        let w = reference_box.width;
        let h = reference_box.height;
        let ox = reference_box.x;
        let oy = reference_box.y;

        match shape {
            GenericBasicShape::Circle(circle) => {
                let (cx, cy) = resolve_position(&circle.position, w, h, ox, oy);
                let r = resolve_shape_radius(&circle.radius, w, h, cx - ox, cy - oy);
                Some(Circle::new(Point::new(cx, cy), r).into_path(0.1))
            }
            GenericBasicShape::Ellipse(ellipse) => {
                let (cx, cy) = resolve_position(&ellipse.position, w, h, ox, oy);
                let rx = resolve_shape_radius(&ellipse.semiaxis_x, w, h, cx - ox, cy - oy);
                let ry = resolve_shape_radius(&ellipse.semiaxis_y, h, w, cy - oy, cx - ox);
                Some(Ellipse::new(Point::new(cx, cy), (rx, ry), 0.0).into_path(0.1))
            }
            GenericBasicShape::Polygon(polygon) => {
                let mut path = BezPath::new();
                let _fill = &polygon.fill;
                let coords = &polygon.coordinates;

                if coords.is_empty() {
                    return None;
                }

                for (i, coord) in coords.iter().enumerate() {
                    let x = ox + resolve_lp(&coord.0, w);
                    let y = oy + resolve_lp(&coord.1, h);
                    if i == 0 {
                        path.move_to(Point::new(x, y));
                    } else {
                        path.line_to(Point::new(x, y));
                    }
                }
                path.close_path();

                Some(path)
            }
            GenericBasicShape::Rect(inset_rect) => {
                // inset() function: inset(top right bottom left round border-radius)
                let top = resolve_lp(&inset_rect.rect.0, h);
                let right = resolve_lp(&inset_rect.rect.1, w);
                let bottom = resolve_lp(&inset_rect.rect.2, h);
                let left = resolve_lp(&inset_rect.rect.3, w);

                let x0 = ox + left;
                let y0 = oy + top;
                let x1 = ox + w - right;
                let y1 = oy + h - bottom;

                if x1 <= x0 || y1 <= y0 {
                    return None;
                }

                // TODO: Support border-radius on inset()
                Some(kurbo::Rect::new(x0, y0, x1, y1).into_path(0.1))
            }
            GenericBasicShape::PathOrShape(path_or_shape) => match path_or_shape {
                GenericPathOrShapeFunction::Path(path) => svg_path_to_bezpath(
                    path.commands(),
                    w,
                    h,
                    |v| *v as f64,
                    |v| *v as f64,
                    |v| *v as f64,
                )
                .map(|mut p| {
                    p.apply_affine(kurbo::Affine::translate((ox, oy)));
                    p
                }),
                GenericPathOrShapeFunction::Shape(shape) => svg_path_to_bezpath(
                    &shape.commands,
                    w,
                    h,
                    move |v: &LengthPercentage| {
                        v.resolve(CSSPixelLength::new(w as f32)).px() as f64
                    },
                    move |v: &LengthPercentage| {
                        v.resolve(CSSPixelLength::new(h as f32)).px() as f64
                    },
                    move |v: &Angle| v.degrees() as f64,
                )
                .map(|mut p| {
                    p.apply_affine(kurbo::Affine::translate((ox, oy)));
                    p
                }),
            },
        }
    }
}

/// A resolved reference box in scaled pixel coordinates
#[derive(Clone, Copy)]
struct ReferenceBox {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl From<ReferenceBox> for kurbo::Rect {
    fn from(reference_box: ReferenceBox) -> Self {
        kurbo::Rect::new(
            reference_box.x,
            reference_box.y,
            reference_box.x + reference_box.width,
            reference_box.y + reference_box.height,
        )
    }
}

/// Resolve a LengthPercentage value against a basis length (already scaled)
fn resolve_lp(lp: &LengthPercentage, basis: f64) -> f64 {
    let basis_css = CSSPixelLength::new(basis as f32);
    lp.resolve(basis_css).px() as f64
}

/// Resolve a position (or auto => center) to absolute coordinates
fn resolve_position(
    position: &GenericPositionOrAuto<style::values::computed::Position>,
    w: f64,
    h: f64,
    ox: f64,
    oy: f64,
) -> (f64, f64) {
    match position {
        GenericPositionOrAuto::Auto => (ox + w / 2.0, oy + h / 2.0),
        GenericPositionOrAuto::Position(pos) => {
            let x = ox + resolve_lp(&pos.horizontal, w);
            let y = oy + resolve_lp(&pos.vertical, h);
            (x, y)
        }
    }
}

/// Resolve a shape radius keyword or length value
fn resolve_shape_radius(
    radius: &GenericShapeRadius<LengthPercentage>,
    primary_size: f64,
    secondary_size: f64,
    center_offset_primary: f64,
    center_offset_secondary: f64,
) -> f64 {
    match radius {
        GenericShapeRadius::Length(lp) => resolve_lp(&lp.0, primary_size),
        GenericShapeRadius::ClosestSide => center_offset_primary
            .min(primary_size - center_offset_primary)
            .min(center_offset_secondary)
            .min(secondary_size - center_offset_secondary)
            .max(0.0),
        GenericShapeRadius::FarthestSide => center_offset_primary
            .max(primary_size - center_offset_primary)
            .max(center_offset_secondary)
            .max(secondary_size - center_offset_secondary),
    }
}

type GenericPathCommand<Angle, N> = GenericShapeCommand<Angle, GenericPosition<N, N>, N>;

/// Convert an SVG path() to a kurbo BezPath.
/// The returned path is in the path's own coordinate system (origin at 0,0).
/// The caller is responsible for translating it to the reference box origin.
fn svg_path_to_bezpath<Angle: Copy, N>(
    commands: &[GenericPathCommand<Angle, N>],
    w: f64,
    h: f64,
    resolve_x: impl Fn(&N) -> f64,
    resolve_y: impl Fn(&N) -> f64,
    resolve_angle: impl Fn(&Angle) -> f64,
) -> Option<BezPath> {
    if commands.is_empty() {
        return None;
    }

    let mut bez = BezPath::new();
    let mut cur = Point::ZERO;
    let mut subpath_start = cur;
    // Tracks the last control point for smooth continuation.
    let mut last_control: Option<Point> = None;

    for cmd in commands {
        match &cmd {
            GenericShapeCommand::Close => {
                bez.close_path();
                cur = subpath_start;
                last_control = None;
            }
            GenericShapeCommand::Move { point } => {
                let p = resolve_endpoint(point, cur, &resolve_x, &resolve_y);
                bez.move_to(p);
                cur = p;
                subpath_start = p;
                last_control = None;
            }
            GenericShapeCommand::Line { point } => {
                let p = resolve_endpoint(point, cur, &resolve_x, &resolve_y);
                bez.line_to(p);
                cur = p;
                last_control = None;
            }
            GenericShapeCommand::HLine { x } => {
                cur.x = resolve_axis_endpoint(x, cur.x, w, &resolve_x);
                bez.line_to(cur);
                last_control = None;
            }
            GenericShapeCommand::VLine { y } => {
                cur.y = resolve_axis_endpoint(y, cur.y, h, &resolve_y);
                bez.line_to(cur);
                last_control = None;
            }
            GenericShapeCommand::CubicCurve {
                point,
                control1,
                control2,
            } => {
                let p = resolve_endpoint(point, cur, &resolve_x, &resolve_y);
                let c1 = resolve_control_point(control1, cur, p, &resolve_x, &resolve_y);
                let c2 = resolve_control_point(control2, cur, p, &resolve_x, &resolve_y);
                bez.curve_to(c1, c2, p);
                last_control = Some(c2);
                cur = p;
            }
            GenericShapeCommand::QuadCurve { point, control1 } => {
                let p = resolve_endpoint(point, cur, &resolve_x, &resolve_y);
                let c1 = resolve_control_point(control1, cur, p, &resolve_x, &resolve_y);
                bez.quad_to(c1, p);
                last_control = Some(c1);
                cur = p;
            }
            GenericShapeCommand::SmoothCubic { point, control2 } => {
                let p = resolve_endpoint(point, cur, &resolve_x, &resolve_y);
                let c2 = resolve_control_point(control2, cur, p, &resolve_x, &resolve_y);
                let c1 = reflect_point(last_control, cur);
                bez.curve_to(c1, c2, p);
                last_control = Some(c2);
                cur = p;
            }
            GenericShapeCommand::SmoothQuad { point } => {
                let p = resolve_endpoint(point, cur, &resolve_x, &resolve_y);
                let c1 = reflect_point(last_control, cur);
                bez.quad_to(c1, p);
                last_control = Some(c1);
                cur = p;
            }
            GenericShapeCommand::Arc {
                point,
                radii,
                arc_sweep,
                arc_size,
                rotate,
            } => {
                // SVG arc commands are complex; approximate as a line for now
                let p = resolve_endpoint(point, cur, &resolve_x, &resolve_y);
                let svg_arc = SvgArc {
                    from: cur,
                    to: p,
                    radii: Vec2 {
                        x: resolve_x(&radii.rx),
                        y: resolve_y(radii.ry.as_ref().unwrap_or(&radii.rx)),
                    },
                    x_rotation: resolve_angle(rotate),
                    large_arc: match arc_size {
                        ArcSize::Large => true,
                        ArcSize::Small => false,
                    },
                    // TODO: check mapping is correct
                    sweep: match arc_sweep {
                        ArcSweep::Ccw => true,
                        ArcSweep::Cw => false,
                    },
                };
                let arc = kurbo::Arc::from_svg_arc(&svg_arc)?;
                for el in arc.append_iter(0.1) {
                    bez.push(el);
                }
                last_control = None;
                cur = p;
            }
        }
    }

    Some(bez)
}

/// Resolve a CommandEndPoint to an absolute Point.
/// `ToPosition` is absolute; `ByCoordinate` is relative to `cur`.
fn resolve_endpoint<N>(
    endpoint: &CommandEndPoint<GenericPosition<N, N>, N>,
    cur: Point,
    resolve_x: impl Fn(&N) -> f64,
    resolve_y: impl Fn(&N) -> f64,
) -> Point {
    match endpoint {
        CommandEndPoint::ToPosition(pos) => {
            Point::new(resolve_x(&pos.horizontal), resolve_y(&pos.vertical))
        }
        CommandEndPoint::ByCoordinate(coord) => {
            Point::new(cur.x + resolve_x(&coord.x), cur.y + resolve_y(&coord.y))
        }
    }
}

/// Resolve a ControlPoint to an absolute Point.
/// - `Absolute`: uses the position directly.
/// - `Relative`: the coordinate pair is offset from a base point determined by `ControlReference`:
///   - `Start` (default): relative to the command's starting point (`cur`).
///   - `End`: relative to the command's end point (`end`).
///   - `Origin`: relative to the reference box origin (0,0 in path coordinates).
fn resolve_control_point<N>(
    control_point: &ControlPoint<GenericPosition<N, N>, N>,
    cur: Point,
    end: Point,
    resolve_x: impl Fn(&N) -> f64,
    resolve_y: impl Fn(&N) -> f64,
) -> Point {
    match control_point {
        ControlPoint::Absolute(pos) => {
            Point::new(resolve_x(&pos.horizontal), resolve_y(&pos.vertical))
        }
        ControlPoint::Relative(rel) => {
            let base = match rel.reference {
                ControlReference::Start => cur,
                ControlReference::End => end,
                ControlReference::Origin => Point::ZERO,
            };
            Point::new(
                base.x + resolve_x(&rel.coord.x),
                base.y + resolve_y(&rel.coord.y),
            )
        }
    }
}

/// Reflect a previous control point around the current point.
/// If there is no previous control point (previous command wasn't the matching curve type),
/// returns the current point itself.
fn reflect_point(last_control: Option<Point>, cur: Point) -> Point {
    match last_control {
        Some(ctrl) => Point::new(2.0 * cur.x - ctrl.x, 2.0 * cur.y - ctrl.y),
        None => cur,
    }
}

/// Resolve an AxisEndPoint to an absolute value.
/// `ToPosition` is absolute; `ByCoordinate` is relative to `cur_val`.
/// `w` and `h` are the reference box dimensions so keywords resolve against the correct axis.
fn resolve_axis_endpoint<N>(
    endpoint: &AxisEndPoint<N>,
    cur_val: f64,
    basis: f64,
    resolve: impl Fn(&N) -> f64,
) -> f64 {
    use style::values::generics::basic_shape::AxisPositionKeyword;
    match endpoint {
        AxisEndPoint::ToPosition(AxisPosition::LengthPercent(lp)) => resolve(lp),
        AxisEndPoint::ToPosition(AxisPosition::Keyword(kw)) => match kw {
            AxisPositionKeyword::Left | AxisPositionKeyword::XStart => 0.0,
            AxisPositionKeyword::Right | AxisPositionKeyword::XEnd => basis,
            AxisPositionKeyword::Top | AxisPositionKeyword::YStart => 0.0,
            AxisPositionKeyword::Bottom | AxisPositionKeyword::YEnd => basis,
            AxisPositionKeyword::Center => basis / 2.0,
        },
        AxisEndPoint::ByCoordinate(val) => cur_val + resolve(val),
    }
}
