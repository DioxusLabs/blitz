//! Basic-shape element -> [`BezPath`] conversion, and the `points` attribute parser.
//!
//! Pure geometry: callers are responsible for pulling resolved `f64` values out of
//! computed style / raw attributes first,
//! so every function here is unit-testable without any Stylo dependency.

use kurbo::{BezPath, Circle, Ellipse, Point, Shape};

/// Tolerance used when flattening kurbo's analytic shapes into cubic Beziers.
/// Small enough to be visually exact at any reasonable zoom; not user-configurable.
const TOLERANCE: f64 = 0.1;

/// Bezier offset ratio for approximating a quarter circle/ ellipse arc with a single cubic curve.
const KAPPA: f64 = 0.5522847498307936;

pub fn resolve_rect_radii(rx: Option<f64>, ry: Option<f64>, width: f64, height: f64) -> (f64, f64) {
    let (rx, ry) = match (rx, ry) {
        (None, None) => (0.0, 0.0),
        (Some(rx), None) => (rx, rx),
        (None, Some(ry)) => (ry, ry),
        (Some(rx), Some(ry)) => (rx, ry),
    };
    (rx.max(0.0).min(width / 2.0), ry.max(0.0).min(height / 2.0))
}

/// `<rect>` geometry. `width`/`height` <= 0 is the "auto -> 0 -> not rendered";
/// the caller checks that before calling this.
pub fn rect_path(x: f64, y: f64, width: f64, height: f64, rx: f64, ry: f64) -> Option<BezPath> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    if rx <= 0.0 || ry <= 0.0 {
        let mut p = BezPath::new();
        p.move_to((x, y));
        p.line_to((x + width, y));
        p.line_to((x + width, y + height));
        p.line_to((x, y + height));
        p.close_path();
        return Some(p);
    }

    let ox = rx * KAPPA;
    let oy = ry * KAPPA;
    let mut p = BezPath::new();
    p.move_to((x + rx, y));
    p.line_to((x + width - rx, y));
    p.curve_to(
        (x + width - rx + ox, y),
        (x + width, y + ry - oy),
        (x + width, y + ry),
    );
    p.line_to((x + width, y + height - ry));
    p.curve_to(
        (x + width, y + height - ry + oy),
        (x + width - rx + ox, y + height),
        (x + width - rx, y + height),
    );
    p.line_to((x + rx, y + height));
    p.curve_to(
        (x + rx - ox, y + height),
        (x, y + height - ry + oy),
        (x, y + height - ry),
    );
    p.line_to((x, y + ry));
    p.curve_to((x, y + ry - oy), (x + rx - ox, y), (x + rx, y));
    p.close_path();
    Some(p)
}

/// `<circle>`. `r <= 0` -> not rendered.
pub fn circle_path(cx: f64, cy: f64, r: f64) -> Option<BezPath> {
    if r <= 0.0 {
        return None;
    }
    Some(Circle::new(Point::new(cx, cy), r).into_path(TOLERANCE))
}

/// `<ellipse>`. `rx <= 0 || ry <= 0` -> not rendered. Auto-radius resolution is the caller's
/// responsibility since it needs the *other* radius already resolved.
pub fn ellipse_path(cx: f64, cy: f64, rx: f64, ry: f64) -> Option<BezPath> {
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    Some(Ellipse::new(Point::new(cx, cy), (rx, ry), 0.0).into_path(TOLERANCE))
}

/// `<line>`. Never filled; stroke-only. Always returns a two-point open subpath regardless of coordinates.
pub fn line_path(x1: f64, y1: f64, x2: f64, y2: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to((x1, y1));
    p.line_to((x2, y2));
    p
}

/// `<polyline>`: open path through `points`. `None` if fewer than 2 points remain after `parse_points`
/// odd-trailing-coordinate handling.
pub fn polyline_path(points: &[(f64, f64)]) -> Option<BezPath> {
    if points.len() < 2 {
        return None;
    }
    let mut p = BezPath::new();
    p.move_to(points[0]);
    for &pt in &points[1..] {
        p.line_to(pt);
    }
    Some(p)
}

/// `<polygon>`: same as `polyline_path` but auto-closed.
pub fn polygon_path(points: &[(f64, f64)]) -> Option<BezPath> {
    let mut p = polyline_path(points)?;
    p.close_path();
    Some(p)
}

/// Parse a `points="x1,y1 x2,y2 ..."` attribute value into coordinate pairs.
/// An odd trailing number (malformed input) is dropped and the  well-formed prefix is kept.
pub fn parse_points(s: &str) -> Vec<(f64, f64)> {
    let nums = super::viewport::parse_number_list(s);
    nums.as_chunks::<2>()
        .0
        .iter()
        .map(|&[x, y]| (x, y))
        .collect()
}

/// `<path d="...">`. Reuses kurbo's own SVG path-data grammar parser (`BezPath::from_svg`) rather than
/// re-implementing or adapting the CSS-`path()`-shapes converter in `blitz-paint::render::clip_path`, which
/// takes a different (already-tokenized, Stylo-typed) input and exists for CSS `clip-path:
/// path(...)` / `shape(...)`, not SVG's `d` grammar.
///
/// Malformed `d` values degrade to "not rendered" rather than a partial path, since a syntax error partway
/// through `d` makes the rest of the string unparseable position-wise.
pub fn path_from_d(d: &str) -> Option<BezPath> {
    if d.trim().is_empty() || d.trim() == "none" {
        return None;
    }
    BezPath::from_svg(d).ok()
}

/// SVG percentage resolution basis: x-axis lengths resolve against viewport width, y-axis against height,
/// and everything else (radii, stroke-width, ...) against the normalized diagonal `sqrt(w^2+h^2) / sqrt(2)`.
pub fn diagonal_basis(viewport_width: f64, viewport_height: f64) -> f64 {
    ((viewport_width * viewport_width + viewport_height * viewport_height) / 2.0).sqrt()
}

/// Parse an SVG length/coordinate/percentage attribute value: a bare number is user units.
pub fn parse_coord(value: &str, basis: f64) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(pct) = value.strip_suffix('%') {
        return pct.trim().parse::<f64>().ok().map(|p| p / 100.0 * basis);
    }
    let numeric_len = value
        .char_indices()
        .take_while(|(i, c)| {
            c.is_ascii_digit()
                || *c == '.'
                || (*c == '-' && *i == 0)
                || (*c == '+' && *i == 0)
                || *c == 'e'
                || *c == 'E'
        })
        .count();
    value[..numeric_len].parse::<f64>().ok()
}

/// `parse_coord` with a `0.0` default for missing/unparseable attributes,
/// for every coordinate attribute covered here
/// (`x`, `y`, `x1`, `y1`, `x2`, `y2`, `dx`, `dy`; `cx`/`cy` also default to `0`).
pub fn parse_coord_or_zero(value: Option<&str>, basis: f64) -> f64 {
    value.and_then(|v| parse_coord(v, basis)).unwrap_or(0.0)
}

/// Parse an SVG `transform` attribute value (the `transform-list` grammar:
/// `translate()`, `scale()`, `rotate()`, `skewX()`, `skewY()`, `matrix()`,
/// space/comma separated, composed left-to-right) into a single [`kurbo::Affine`].
pub fn parse_transform_list(value: &str) -> kurbo::Affine {
    let mut result = kurbo::Affine::IDENTITY;
    let mut rest = value.trim();
    while !rest.is_empty() {
        let Some(open) = rest.find('(') else { break };
        let name = rest[..open].trim();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args_str = &rest[open + 1..open + close];
        let args = super::viewport::parse_number_list(args_str);
        rest = rest[open + close + 1..].trim_start_matches([' ', ',']);

        let m = match (name, args.as_slice()) {
            ("translate", [x]) => Some(kurbo::Affine::translate((*x, 0.0))),
            ("translate", [x, y]) => Some(kurbo::Affine::translate((*x, *y))),
            ("scale", [s]) => Some(kurbo::Affine::scale(*s)),
            ("scale", [sx, sy]) => Some(kurbo::Affine::scale_non_uniform(*sx, *sy)),
            ("rotate", [deg]) => Some(kurbo::Affine::rotate(deg.to_radians())),
            ("rotate", [deg, cx, cy]) => Some(
                kurbo::Affine::translate((*cx, *cy))
                    * kurbo::Affine::rotate(deg.to_radians())
                    * kurbo::Affine::translate((-*cx, -*cy)),
            ),
            ("skewX", [deg]) => Some(kurbo::Affine::skew(deg.to_radians().tan(), 0.0)),
            ("skewY", [deg]) => Some(kurbo::Affine::skew(0.0, deg.to_radians().tan())),
            ("matrix", [a, b, c, d, e, f]) => Some(kurbo::Affine::new([*a, *b, *c, *d, *e, *f])),
            _ => None,
        };
        if let Some(m) = m {
            result *= m;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_radii_both_auto_are_zero() {
        assert_eq!(resolve_rect_radii(None, None, 100.0, 50.0), (0.0, 0.0));
    }

    #[test]
    fn rect_radii_one_auto_takes_the_other() {
        assert_eq!(
            resolve_rect_radii(Some(10.0), None, 100.0, 50.0),
            (10.0, 10.0)
        );
        assert_eq!(resolve_rect_radii(None, Some(8.0), 100.0, 50.0), (8.0, 8.0));
    }

    #[test]
    fn rect_radii_clamp_to_half_axis() {
        assert_eq!(
            resolve_rect_radii(Some(1000.0), Some(1000.0), 100.0, 50.0),
            (50.0, 25.0)
        );
    }

    #[test]
    fn zero_or_negative_rect_dims_are_not_rendered() {
        assert!(rect_path(0.0, 0.0, 0.0, 10.0, 0.0, 0.0).is_none());
        assert!(rect_path(0.0, 0.0, 10.0, -1.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn zero_radius_circle_is_not_rendered() {
        assert!(circle_path(0.0, 0.0, 0.0).is_none());
        assert!(circle_path(0.0, 0.0, -5.0).is_none());
        assert!(circle_path(0.0, 0.0, 5.0).is_some());
    }

    #[test]
    fn degenerate_ellipse_is_not_rendered() {
        assert!(ellipse_path(0.0, 0.0, 0.0, 5.0).is_none());
        assert!(ellipse_path(0.0, 0.0, 5.0, 0.0).is_none());
    }

    #[test]
    fn parse_points_drops_odd_trailing_coordinate() {
        assert_eq!(
            parse_points("0,0 10,10 20,20 5"),
            vec![(0.0, 0.0), (10.0, 10.0), (20.0, 20.0)]
        );
    }

    #[test]
    fn parse_points_handles_whitespace_separated() {
        assert_eq!(parse_points("0 0 10 10"), vec![(0.0, 0.0), (10.0, 10.0)]);
    }

    #[test]
    fn polyline_is_open_polygon_is_closed() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let polyline = polyline_path(&pts).unwrap();
        let polygon = polygon_path(&pts).unwrap();
        assert!(!matches!(
            polyline.elements().last(),
            Some(kurbo::PathEl::ClosePath)
        ));
        assert!(matches!(
            polygon.elements().last(),
            Some(kurbo::PathEl::ClosePath)
        ));
    }

    #[test]
    fn too_few_points_yields_no_path() {
        assert!(polyline_path(&[(0.0, 0.0)]).is_none());
    }

    #[test]
    fn parse_coord_handles_bare_numbers_and_percent() {
        assert_eq!(parse_coord("10", 200.0), Some(10.0));
        assert_eq!(parse_coord("-5.5", 200.0), Some(-5.5));
        assert_eq!(parse_coord("50%", 200.0), Some(100.0));
        assert_eq!(parse_coord("", 200.0), None);
    }

    #[test]
    fn parse_coord_or_zero_defaults_missing_to_zero() {
        assert_eq!(parse_coord_or_zero(None, 200.0), 0.0);
        assert_eq!(parse_coord_or_zero(Some("bogus"), 200.0), 0.0);
    }

    #[test]
    fn transform_list_composes_translate_and_scale() {
        let t = parse_transform_list("translate(10, 20) scale(2)");
        let p = t * Point::new(0.0, 0.0);
        assert_eq!(p, Point::new(10.0, 20.0));
        let p2 = t * Point::new(1.0, 1.0);
        assert_eq!(p2, Point::new(12.0, 22.0));
    }

    #[test]
    fn transform_list_skips_unknown_function() {
        let t = parse_transform_list("bogus(1,2,3) translate(5,5)");
        assert_eq!(t * Point::new(0.0, 0.0), Point::new(5.0, 5.0));
    }

    #[test]
    fn path_from_d_parses_simple_path() {
        let p = path_from_d("M0,0 L10,0 L10,10 Z").unwrap();
        assert!(p.elements().len() >= 3);
    }

    #[test]
    fn path_from_d_rejects_empty_and_none() {
        assert!(path_from_d("").is_none());
        assert!(path_from_d("none").is_none());
    }

    #[test]
    fn diagonal_basis_matches_svg_normalized_diagonal_formula() {
        // sqrt((w^2+h^2)/2), e.g. a square viewport's diagonal basis equals its side length.
        assert!((diagonal_basis(100.0, 100.0) - 100.0).abs() < 1e-9);
    }
}
