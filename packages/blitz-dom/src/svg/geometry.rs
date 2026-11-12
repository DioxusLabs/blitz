//! Shape geometry and stroke styles for native SVG layout.

use kurbo::{BezPath, Line, ParamCurveArclen, PathSeg, Point, Rect, Shape, Stroke};
use markup5ever::local_name;
use style::computed_values::stroke_linecap::T as StrokeLinecap;
use style::computed_values::stroke_linejoin::T as StrokeLinejoin;
use style::properties::ComputedValues;
use style::values::CSSFloat;
use style::values::computed::svg::{SVGLength, SVGStrokeDashArray, SVGWidth};
use style::values::computed::{NonNegativeLengthPercentageOrAuto, Size};
use style::values::specified::svg::DProperty;

use super::{SvgViewport, attr};
use crate::node::ElementData;
use crate::svg_path_to_bezpath;

/// Tolerance for flattening curves into Bézier paths, in user units.
const TOLERANCE: f64 = 0.1;
/// Accuracy of `pathLength` arc-length measurement, in user units.
const ARCLEN_ACCURACY: f64 = 1e-3;

/// Parse a length attribute that is not a CSS property (`x1`, `y1`, ...),
/// resolving percentages against `basis`.
fn attr_length(element: &ElementData, name: &str, basis: f64, font_size: f64) -> Option<f64> {
    let len: svgtypes::Length = attr(element, name)?.trim().parse().ok()?;
    Some(resolve_svg_length(len, basis, font_size))
}

/// Resolve a length parsed from an attribute to user units.
pub(super) fn resolve_svg_length(len: svgtypes::Length, basis: f64, font_size: f64) -> f64 {
    use svgtypes::LengthUnit as U;
    let n = len.number;
    match len.unit {
        U::None | U::Px => n,
        U::Percent => n / 100.0 * basis,
        U::Em => n * font_size,
        U::Ex => n * font_size / 2.0,
        U::In => n * 96.0,
        U::Cm => n * 96.0 / 2.54,
        U::Mm => n * 96.0 / 25.4,
        U::Pt => n * 4.0 / 3.0,
        U::Pc => n * 16.0,
    }
}

pub(super) fn font_size(style: &ComputedValues) -> f64 {
    style.get_font().font_size.computed_size().px() as f64
}

/// The outline of a basic shape or `<path>`, in user units. `None` when the
/// element is not a shape or the shape is disabled (zero size, no data).
/// <https://svgwg.org/svg2-draft/shapes.html>
pub(super) fn shape_path(
    tag: &markup5ever::LocalName,
    element: &ElementData,
    style: &ComputedValues,
    viewport: SvgViewport,
) -> Option<BezPath> {
    let svg = style.get_svg();
    let radius = |r: &NonNegativeLengthPercentageOrAuto, basis: f64| match r {
        NonNegativeLengthPercentageOrAuto::LengthPercentage(lp) => {
            Some(viewport.resolve(&lp.0, basis))
        }
        NonNegativeLengthPercentageOrAuto::Auto => None,
    };

    if *tag == local_name!("rect") {
        let size = |size: &Size, basis: f64| match size {
            Size::LengthPercentage(lp) => viewport.resolve(&lp.0, basis),
            _ => 0.0,
        };
        let width = size(&style.get_position().width, viewport.width);
        let height = size(&style.get_position().height, viewport.height);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let x = viewport.x(&svg.x);
        let y = viewport.y(&svg.y);
        let (rx, ry) = match (
            radius(&svg.rx, viewport.width),
            radius(&svg.ry, viewport.height),
        ) {
            (Some(rx), Some(ry)) => (rx, ry),
            (Some(r), None) | (None, Some(r)) => (r, r),
            (None, None) => (0.0, 0.0),
        };
        let rect = Rect::new(x, y, x + width, y + height);
        if rx > 0.0 && ry > 0.0 {
            return Some(rounded_rect(
                rect,
                rx.min(width / 2.0),
                ry.min(height / 2.0),
            ));
        }
        return Some(rect_path(rect));
    }
    if *tag == local_name!("circle") {
        let r = viewport.other(&svg.r.0);
        if r <= 0.0 {
            return None;
        }
        let center = Point::new(viewport.x(&svg.cx), viewport.y(&svg.cy));
        return Some(ellipse_path(center, r, r));
    }
    if *tag == local_name!("ellipse") {
        let (rx, ry) = match (
            radius(&svg.rx, viewport.width),
            radius(&svg.ry, viewport.height),
        ) {
            (Some(rx), Some(ry)) => (rx, ry),
            (Some(r), None) | (None, Some(r)) => (r, r),
            (None, None) => return None,
        };
        if rx <= 0.0 || ry <= 0.0 {
            return None;
        }
        let center = Point::new(viewport.x(&svg.cx), viewport.y(&svg.cy));
        return Some(ellipse_path(center, rx, ry));
    }
    if *tag == local_name!("line") {
        let fs = viewport.unzoom(font_size(style));
        let len = |name, basis| attr_length(element, name, basis, fs).unwrap_or(0.0);
        let p0 = Point::new(len("x1", viewport.width), len("y1", viewport.height));
        let p1 = Point::new(len("x2", viewport.width), len("y2", viewport.height));
        return Some(Line::new(p0, p1).to_path(TOLERANCE));
    }
    if *tag == local_name!("polyline") || *tag == local_name!("polygon") {
        // Rendering stops at the first error, and an odd trailing coordinate
        // is dropped: both are what `PointsParser` does.
        let mut path = BezPath::new();
        for (i, (x, y)) in svgtypes::PointsParser::from(attr(element, "points")?).enumerate() {
            if i == 0 {
                path.move_to((x, y));
            } else {
                path.line_to((x, y));
            }
        }
        if path.elements().is_empty() {
            return None;
        }
        if *tag == local_name!("polygon") {
            path.close_path();
        }
        return Some(path);
    }
    if *tag == local_name!("path") {
        let DProperty::Path(data) = &svg.d else {
            return None;
        };
        return svg_path_to_bezpath(
            data.commands(),
            |v: &CSSFloat| *v as f64,
            |v: &CSSFloat| *v as f64,
            |v: &CSSFloat| *v as f64,
        );
    }
    None
}

/// An ellipse path starting at its rightmost point and going through
/// `(cx, cy + ry)` first, as the spec says (which matters for dashing).
/// <https://svgwg.org/svg2-draft/shapes.html#CircleElement>
fn ellipse_path(center: Point, rx: f64, ry: f64) -> BezPath {
    let arc = kurbo::Arc::new(center, (rx, ry), 0.0, std::f64::consts::TAU, 0.0);
    let mut path = BezPath::new();
    path.move_to(center + kurbo::Vec2::new(rx, 0.0));
    arc.to_cubic_beziers(TOLERANCE, |p1, p2, p| path.curve_to(p1, p2, p));
    path.close_path();
    path
}

/// A rectangle path starting at the top-left corner and going clockwise, as
/// the SVG spec defines it (which matters for dashing).
fn rect_path(rect: Rect) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((rect.x0, rect.y0));
    path.line_to((rect.x1, rect.y0));
    path.line_to((rect.x1, rect.y1));
    path.line_to((rect.x0, rect.y1));
    path.close_path();
    path
}

/// A rectangle with elliptical corners, following the path the spec gives
/// for `<rect>`. <https://svgwg.org/svg2-draft/shapes.html#RectElement>
fn rounded_rect(rect: Rect, rx: f64, ry: f64) -> BezPath {
    let mut path = BezPath::new();
    let quarter = |path: &mut BezPath, center: Point, start: f64| {
        let arc = kurbo::Arc::new(center, (rx, ry), start, std::f64::consts::FRAC_PI_2, 0.0);
        arc.to_cubic_beziers(TOLERANCE, |p1, p2, p| path.curve_to(p1, p2, p));
    };
    use std::f64::consts::{FRAC_PI_2, PI};
    path.move_to((rect.x0 + rx, rect.y0));
    path.line_to((rect.x1 - rx, rect.y0));
    quarter(
        &mut path,
        Point::new(rect.x1 - rx, rect.y0 + ry),
        -FRAC_PI_2,
    );
    path.line_to((rect.x1, rect.y1 - ry));
    quarter(&mut path, Point::new(rect.x1 - rx, rect.y1 - ry), 0.0);
    path.line_to((rect.x0 + rx, rect.y1));
    quarter(&mut path, Point::new(rect.x0 + rx, rect.y1 - ry), FRAC_PI_2);
    path.line_to((rect.x0, rect.y0 + ry));
    quarter(&mut path, Point::new(rect.x0 + rx, rect.y0 + ry), PI);
    path.close_path();
    path
}

fn path_length(path: &BezPath) -> f64 {
    path.segments()
        .map(|seg: PathSeg| seg.arclen(ARCLEN_ACCURACY))
        .sum()
}

/// The stroke style for a shape, or `None` when it has no visible stroke
/// geometry (`stroke-width: 0`, or a `pathLength` of zero with dashes that
/// leave nothing to draw). Whether the stroke has a paint is up to the painter.
pub(super) fn stroke(
    element: &ElementData,
    style: &ComputedValues,
    viewport: SvgViewport,
    path: &BezPath,
) -> Option<Stroke> {
    let svg = style.get_inherited_svg();
    let width = match &svg.stroke_width {
        SVGWidth::LengthPercentage(lp) => viewport.other(&lp.0),
        SVGWidth::ContextValue => 1.0,
    };
    if width.is_nan() || width <= 0.0 {
        return None;
    }
    let mut stroke = Stroke::new(width)
        .with_caps(match svg.stroke_linecap {
            StrokeLinecap::Butt => kurbo::Cap::Butt,
            StrokeLinecap::Round => kurbo::Cap::Round,
            StrokeLinecap::Square => kurbo::Cap::Square,
        })
        .with_join(match svg.stroke_linejoin {
            StrokeLinejoin::Miter => kurbo::Join::Miter,
            StrokeLinejoin::Round => kurbo::Join::Round,
            StrokeLinejoin::Bevel => kurbo::Join::Bevel,
        })
        .with_miter_limit(svg.stroke_miterlimit.0 as f64);

    let SVGStrokeDashArray::Values(values) = &svg.stroke_dasharray else {
        return Some(stroke);
    };
    let mut dashes: Vec<f64> = values.iter().map(|lp| viewport.other(&lp.0)).collect();
    // A dash array that is empty, has a negative value or sums to zero
    // renders the stroke solid. https://svgwg.org/svg2-draft/painting.html#StrokeDasharrayProperty
    if dashes.is_empty()
        || dashes.iter().any(|d| !d.is_finite() || *d < 0.0)
        || dashes.iter().sum::<f64>() <= 0.0
    {
        return Some(stroke);
    }
    if dashes.len() % 2 == 1 {
        dashes.extend_from_within(..);
    }
    let mut offset = match &svg.stroke_dashoffset {
        SVGLength::LengthPercentage(lp) => viewport.other(lp),
        SVGLength::ContextValue => 0.0,
    };

    // `pathLength` scales dash lengths and offsets from the author's length
    // to the computed one. https://svgwg.org/svg2-draft/paths.html#PathLengthAttribute
    let author_length = attr(element, "pathLength")
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0);
    if let Some(author_length) = author_length {
        if author_length == 0.0 {
            // An infinite scale: the first non-zero entry covers the path.
            // A leading dash makes it solid; a leading gap hides it.
            let first = dashes.iter().position(|d| *d > 0.0).unwrap();
            return (first % 2 == 0).then_some(stroke);
        }
        let scale = path_length(path) / author_length;
        dashes.iter_mut().for_each(|d| *d *= scale);
        offset *= scale;
    }
    stroke = stroke.with_dashes(offset, dashes);
    Some(stroke)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_rect_starts_after_the_top_left_corner() {
        let path = rounded_rect(Rect::new(0.0, 0.0, 100.0, 50.0), 10.0, 5.0);
        let bbox = path.bounding_box();
        assert!((bbox.width() - 100.0).abs() < 1e-6 && (bbox.height() - 50.0).abs() < 1e-6);
        assert_eq!(
            path.elements()[0],
            kurbo::PathEl::MoveTo(Point::new(10.0, 0.0))
        );
    }

    #[test]
    fn path_length_of_a_rect() {
        let len = path_length(&rect_path(Rect::new(0.0, 0.0, 30.0, 20.0)));
        assert!((len - 100.0).abs() < 1e-6);
    }
}
