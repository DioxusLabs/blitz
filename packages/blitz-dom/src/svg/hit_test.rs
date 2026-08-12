//! SVG hit testing.
//!
//! `pointer-events` on this Stylo build's servo engine has only `Auto | None`.

use kurbo::{Point, Shape, Stroke};

use super::context::{SvgContext, SvgNodeKind};
use style::properties::generated::longhands::visibility::computed_value::T as StyloVisibility;

/// Whether an SVG paint value counts as "painted" for hit testing purposes.
fn is_painted(raw_paint_attr: Option<&str>, default_none: bool) -> bool {
    match raw_paint_attr.map(str::trim) {
        Some("none") => false,
        Some(_) => true,
        None => !default_none,
    }
}

/// Hit-test `point` against `ctx`, walking nodes in reverse render order.
/// Returns the DOM id of the topmost hit shape, if any.
pub fn hit_test(
    tree_node: &crate::Node,
    ctx: &SvgContext,
    point: Point,
) -> Option<blitz_traits::node_id::NodeId> {
    for node in ctx.nodes.iter().rev() {
        let SvgNodeKind::Shape(path) = &node.kind else {
            continue;
        };

        let Some(inverse) = try_invert(node.ctm) else {
            continue;
        };
        let local_point = inverse * point;

        let dom_node = tree_node.with(node.dom_id);
        let attrs = dom_node.attrs().unwrap_or(&[]);

        if let Some(style) = dom_node.primary_styles() {
            if style.get_inherited_box().visibility != StyloVisibility::Visible {
                continue;
            }
        }

        if super::attrs::raw_attr(attrs, "pointer-events") == Some("none") {
            continue;
        }

        let fill_painted = is_painted(super::attrs::raw_attr(attrs, "fill"), false);
        let stroke_painted = is_painted(super::attrs::raw_attr(attrs, "stroke"), true);

        if fill_painted && path.contains(local_point) {
            return Some(node.dom_id);
        }

        if stroke_painted {
            let stroke_width = super::attrs::raw_attr(attrs, "stroke-width")
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(1.0);
            if stroke_width > 0.0 {
                let stroke = Stroke::new(stroke_width);
                let outline = kurbo::stroke(
                    path.path_elements(0.1),
                    &stroke,
                    &kurbo::StrokeOpts::default(),
                    0.1,
                );
                if outline.contains(local_point) {
                    return Some(node.dom_id);
                }
            }
        }
    }
    None
}

fn try_invert(affine: kurbo::Affine) -> Option<kurbo::Affine> {
    // kurbo::Affine::inverse() is defined for all (including singular)
    // matrices, producing non-finite output for singular ones; guard
    // explicitly rather than hit-testing against garbage coordinates.
    if affine.determinant().abs() < 1e-12 {
        return None;
    }
    Some(affine.inverse())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_fill_defaults_to_painted() {
        assert!(is_painted(None, false));
    }

    #[test]
    fn unset_stroke_defaults_to_not_painted() {
        assert!(!is_painted(None, true));
    }

    #[test]
    fn explicit_none_is_never_painted() {
        assert!(!is_painted(Some("none"), false));
        assert!(!is_painted(Some("none"), true));
    }

    #[test]
    fn explicit_color_is_painted() {
        assert!(is_painted(Some("red"), true));
    }

    #[test]
    fn singular_matrix_is_not_invertible_for_hit_testing() {
        let singular = kurbo::Affine::new([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(try_invert(singular).is_none());
        let identity = kurbo::Affine::IDENTITY;
        assert!(try_invert(identity).is_some());
    }
}
