use super::ElementCx;
pub(super) use blitz_dom::svg_path_to_bezpath;
use kurbo::{BezPath, Circle, Ellipse, Point, Rect, Shape};
use style::values::computed::basic_shape::{BasicShape, ClipPath};
use style::values::computed::{Angle, CSSPixelLength, LengthPercentage};
use style::values::generics::basic_shape::{
    GenericBasicShape, GenericPathOrShapeFunction, GenericShapeRadius, ShapeBox, ShapeGeometryBox,
};
use style::values::generics::position::GenericPositionOrAuto;

impl ElementCx<'_, '_> {
    /// Compute the rectangle (in scaled pixels, relative to the border box origin) described by
    /// the CSS 2 `clip` property, if it applies. `clip` only applies to absolutely positioned
    /// elements; `auto` components resolve to the corresponding border box edge.
    pub(super) fn css_clip_rect(&self) -> Option<Rect> {
        use style::computed_values::position::T as Position;
        use style::values::computed::LengthOrAuto;
        use style::values::generics::ClipRectOrAuto;

        if !matches!(
            self.style.get_box().position,
            Position::Absolute | Position::Fixed
        ) {
            return None;
        }
        let ClipRectOrAuto::Rect(rect) = &self.style.get_effects().clip else {
            return None;
        };

        let border_box = self.frame.border_box;
        let resolve = |component: &LengthOrAuto, auto: f64| match component {
            LengthOrAuto::Auto => auto,
            LengthOrAuto::LengthPercentage(length) => length.px() as f64 * self.scale,
        };
        let x0 = resolve(&rect.left, 0.0);
        let y0 = resolve(&rect.top, 0.0);
        let x1 = resolve(&rect.right, border_box.width());
        let y1 = resolve(&rect.bottom, border_box.height());

        Some(Rect::new(x0, y0, x1.max(x0), y1.max(y0)))
    }

    /// Compute the clip-path BezPath (if any) for this element.
    /// Returns `None` if clip-path is `none` or unsupported.
    pub(super) fn clip_path_shape(&self) -> Option<BezPath> {
        match self.style.get_clip_path() {
            ClipPath::None => None,
            ClipPath::Url(_) => {
                // URL references (e.g. `clip-path: url(#myClip)`) are not yet supported
                None
            }
            ClipPath::Shape(basic_shape, geometry_box) => {
                let reference_box = self.resolve_geometry_box(geometry_box);
                self.basic_shape_to_path(basic_shape, reference_box)
            }
            ClipPath::Box(geometry_box) => {
                let reference_box = self.resolve_geometry_box(geometry_box);
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
            ShapeGeometryBox::ShapeBox(ShapeBox::PaddingBox) => {
                let padding_box = self.frame.padding_box();
                ReferenceBox {
                    x: self.frame.border_width.x0 / self.scale,
                    y: self.frame.border_width.y0 / self.scale,
                    width: padding_box.width() / self.scale,
                    height: padding_box.height() / self.scale,
                }
            }
            ShapeGeometryBox::FillBox | ShapeGeometryBox::ShapeBox(ShapeBox::ContentBox) => {
                let content_box = self.frame.content_box();
                ReferenceBox {
                    x: (self.frame.border_width.x0 + self.frame.padding_width.x0) / self.scale,
                    y: (self.frame.border_width.y0 + self.frame.padding_width.y0) / self.scale,
                    width: content_box.width() / self.scale,
                    height: content_box.height() / self.scale,
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

        GenericShapeRadius::FarthestCorner => {
            let dp = center_offset_primary.max(primary_size - center_offset_primary);
            let ds = center_offset_secondary.max(secondary_size - center_offset_secondary);
            dp.hypot(ds)
        }
        GenericShapeRadius::ClosestCorner => {
            let dp = center_offset_primary
                .min(primary_size - center_offset_primary)
                .max(0.0);
            let ds = center_offset_secondary
                .min(secondary_size - center_offset_secondary)
                .max(0.0);
            dp.hypot(ds)
        }
    }
}
