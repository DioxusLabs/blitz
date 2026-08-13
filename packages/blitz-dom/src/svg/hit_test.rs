//! SVG hit testing.
//!
//! `pointer-events` on this Stylo build's servo engine has only `Auto | None`.

use kurbo::{Point, Shape, Stroke};

use super::context::{SvgContext, SvgNodeKind};
use style::properties::generated::longhands::visibility::computed_value::T as StyloVisibility;

/// Whether a computed SVG paint counts as "painted" for hit testing purposes.
/// Stylo already resolves the SVG initial values into a concrete `SVGPaintKind`, so there's no
/// separate "unset" case to handle here the way there was for raw attribute strings.
fn is_painted(paint: &style::values::computed::svg::SVGPaint) -> bool {
    use style::values::computed::svg::SVGPaintKind;
    use style::values::generics::svg::SVGPaintFallback;
    match &paint.kind {
        SVGPaintKind::None => false,
        SVGPaintKind::Color(_) => true,
        SVGPaintKind::PaintServer(_) => matches!(paint.fallback, SVGPaintFallback::Color(_)),
        SVGPaintKind::ContextFill | SVGPaintKind::ContextStroke => false,
    }
}

/// Computed `stroke-width` resolved to user units (px), mirroring `blitz_paint::render::svg::svg_length_px`.
fn stroke_width_px(width: &style::values::computed::svg::SVGWidth, viewport: kurbo::Size) -> f64 {
    use style::values::computed::length::Length;
    use style::values::computed::length_percentage::Unpacked;
    use style::values::computed::svg::SVGWidth;
    let diag = super::geometry::diagonal_basis(viewport.width, viewport.height);
    match width {
        SVGWidth::LengthPercentage(lp) => match lp.0.unpack() {
            Unpacked::Length(l) => l.px() as f64,
            Unpacked::Percentage(p) => p.0 as f64 * diag,
            Unpacked::Calc(c) => c.resolve(Length::new(diag as f32)).px() as f64,
        },
        SVGWidth::ContextValue => 1.0,
    }
}

/// Hit-test `point` against `ctx`, walking nodes in reverse render order.
/// Returns the DOM id of the topmost hit shape, if any.
pub fn hit_test(
    tree_node: &crate::Node,
    ctx: &SvgContext,
    point: Point,
) -> Option<blitz_traits::node_id::NodeId> {
    use style::computed_values::pointer_events::T as PointerEvents;

    for node in ctx.nodes.iter().rev() {
        let SvgNodeKind::Shape(path) = &node.kind else {
            continue;
        };

        let Some(inverse) = try_invert(node.ctm) else {
            continue;
        };
        let local_point = inverse * point;

        let dom_node = tree_node.with(node.dom_id);
        let Some(style) = dom_node.primary_styles() else {
            continue;
        };

        if style.get_inherited_box().visibility != StyloVisibility::Visible {
            continue;
        }
        if style.clone_pointer_events() == PointerEvents::None {
            continue;
        }

        let svg = style.get_inherited_svg();
        let fill_painted = is_painted(&svg.fill);
        let stroke_painted = is_painted(&svg.stroke);

        if fill_painted && path.contains(local_point) {
            return Some(node.dom_id);
        }

        if stroke_painted {
            let stroke_width = stroke_width_px(&svg.stroke_width, ctx.viewport);
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
    use style::color::AbsoluteColor;
    use style::values::computed::color::Color as ComputedColor;
    use style::values::computed::svg::{SVGPaint, SVGPaintKind};
    use style::values::generics::svg::SVGPaintFallback;

    fn color_paint() -> SVGPaint {
        SVGPaint {
            kind: SVGPaintKind::Color(ComputedColor::Absolute(AbsoluteColor::BLACK)),
            fallback: SVGPaintFallback::Unset,
        }
    }

    #[test]
    fn none_paint_is_never_painted() {
        let none = SVGPaint {
            kind: SVGPaintKind::None,
            fallback: SVGPaintFallback::Unset,
        };
        assert!(!is_painted(&none));
    }

    #[test]
    fn color_paint_is_painted() {
        assert!(is_painted(&color_paint()));
    }

    #[test]
    fn fill_initial_value_is_opaque_black_and_painted() {
        assert!(is_painted(&SVGPaint::BLACK));
    }

    #[test]
    fn singular_matrix_is_not_invertible_for_hit_testing() {
        let singular = kurbo::Affine::new([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(try_invert(singular).is_none());
        let identity = kurbo::Affine::IDENTITY;
        assert!(try_invert(identity).is_some());
    }
}
