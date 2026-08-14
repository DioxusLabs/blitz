//! Paint a first-party inline `<svg>` fragment.

use anyrender::PaintScene;
use blitz_dom::svg::SvgNodeKind;
use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{Color, Fill};

use crate::color::ToColorColor;

use super::ElementCx;

/// One entry in the DFS-path stack `draw_svg_fragment` walks alongside the flat, pre-order
/// `ctx.nodes` list. `ctx.nodes` is exactly a pre-order traversal with parent indices, so a
/// node's subtree is a contiguous run: popping frames whose index isn't the next node's parent
/// closes exactly the subtrees that have been fully visited, no ancestor-chain walk needed.
struct Frame {
    idx: u32,
    layer_open: bool,
}

impl ElementCx<'_, '_> {
    pub(super) fn draw_svg_fragment(&self, scene: &mut impl PaintScene) {
        let Some(ctx) = self.svg_root else {
            return;
        };

        let base = self.transform * Affine::scale(self.scale);
        let doc = self.node;
        let viewport = ctx.viewport;
        // Clip source for a *group's* opacity layer: groups have no accumulated bounding box, so clip to
        // the fragment's own viewport in base space instead. Always finite, never over-tight enough to clip real content.
        let fragment_clip =
            base * Rect::new(0.0, 0.0, viewport.width, viewport.height).to_path(0.1);

        let mut stack: Vec<Frame> = Vec::new();

        for (i, node) in ctx.nodes.iter().enumerate() {
            let i = i as u32;

            while let Some(top) = stack.last() {
                if node.parent == Some(top.idx) {
                    break;
                }
                let top = stack.pop().unwrap();
                if top.layer_open {
                    scene.pop_layer();
                }
            }

            let dom_node = doc.with(node.dom_id);
            let ctm = base * node.ctm;

            use style::properties::generated::longhands::visibility::computed_value::T as StyloVisibility;
            let style = dom_node.primary_styles();
            let is_leaf = matches!(
                node.kind,
                SvgNodeKind::Shape(_) | SvgNodeKind::Text(_) | SvgNodeKind::ForeignObject
            );
            let visible = style
                .as_ref()
                .is_none_or(|s| s.get_inherited_box().visibility == StyloVisibility::Visible);

            // A hidden leaf paints nothing, so skip it outright. A hidden *group* still pushes a frame -- `visibility`
            // is inherited and a descendant may override it back to visible.
            if is_leaf && !visible {
                continue;
            }

            let opacity = style
                .as_ref()
                .map(|s| s.get_effects().opacity)
                .unwrap_or(1.0);
            let finite = ctm.is_finite() && node.bbox.is_finite();
            let mut layer_open = false;

            if finite && opacity < 1.0 {
                // `opacity <= 0.0` still opens a (zero-alpha) layer rather than being
                // special-cased into a skip: a group's descendants are separate entries
                // later in this flat pre-order list, so skipping only the group's own
                // frame would leave them to paint at full opacity on their own iterations.
                let clip = if is_leaf {
                    let stroke_pad = style
                        .as_ref()
                        .map(|s| svg_length_px(&s.get_inherited_svg().stroke_width, viewport) / 2.0)
                        .unwrap_or(0.0);
                    ctm * node.bbox.inflate(stroke_pad, stroke_pad).to_path(0.1)
                } else {
                    fragment_clip.clone()
                };
                scene.push_layer(
                    peniko::Mix::Normal,
                    opacity.max(0.0),
                    Affine::IDENTITY,
                    &clip,
                    None,
                    None,
                );
                layer_open = true;
            }

            if !finite {
                stack.push(Frame {
                    idx: i,
                    layer_open: false,
                });
                continue;
            }

            match &node.kind {
                SvgNodeKind::Shape(path) => {
                    if let Some(style) = &style {
                        paint_shape(scene, ctm, path, style, viewport);
                    }
                }
                SvgNodeKind::Text(run) => {
                    let current_color = style
                        .as_ref()
                        .map(|s| resolve_current_color(s))
                        .unwrap_or(Color::BLACK);
                    self.draw_svg_text(scene, ctm, run, current_color);
                }
                SvgNodeKind::ForeignObject => {
                    let unbounded = kurbo::Rect::new(f64::MIN, f64::MIN, f64::MAX, f64::MAX);
                    for &child in dom_node.children.iter() {
                        self.context.render_node(scene, child, ctm, unbounded);
                    }
                }
                SvgNodeKind::Group | SvgNodeKind::Use { .. } | SvgNodeKind::Image => {}
            }

            stack.push(Frame { idx: i, layer_open });
        }

        while let Some(top) = stack.pop() {
            if top.layer_open {
                scene.pop_layer();
            }
        }
    }

    fn draw_svg_text(
        &self,
        scene: &mut impl PaintScene,
        ctm: Affine,
        run: &blitz_dom::svg::TextRun,
        current_color: Color,
    ) {
        use blitz_dom::svg::text::TextAnchor;
        let full_width = run.layout.full_width();
        let anchor_dx = match run.anchor {
            TextAnchor::Start => 0.0,
            TextAnchor::Middle => -full_width / 2.0,
            TextAnchor::End => -full_width,
        } as f64;
        let text_transform = ctm * Affine::translate((anchor_dx, 0.0));

        for line in run.layout.lines() {
            for item in line.items() {
                let parley::PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run_ref = glyph_run.run();
                let font = run_ref.font();
                let font_size = run_ref.font_size();
                let glyph_xform: Option<kurbo::Affine> = None;
                let coords = run_ref.normalized_coords();

                let mut x = glyph_run.offset() as f64;
                let y = glyph_run.baseline() as f64;
                let glyphs = glyph_run.glyphs().map(move |g| {
                    let gx = x + g.x as f64;
                    x += g.advance as f64;
                    anyrender::Glyph {
                        id: g.id,
                        x: gx as f32,
                        y: y as f32,
                    }
                });

                scene.draw_glyphs(
                    font,
                    font_size,
                    true,
                    coords,
                    kurbo::Vec2::ZERO,
                    Fill::NonZero,
                    current_color,
                    1.0,
                    text_transform,
                    glyph_xform,
                    glyphs,
                );
            }
        }
    }
}

/// Paint `path`'s fill/stroke from *computed* SVG style, not raw DOM attributes: presentation
/// attributes on an ancestor cascade down and only the computed value sees the inherited result,
/// and author CSS/`:hover` overrides only take effect here too.
fn paint_shape(
    scene: &mut impl PaintScene,
    ctm: Affine,
    path: &kurbo::BezPath,
    style: &style::properties::ComputedValues,
    viewport: kurbo::Size,
) {
    use style::values::computed::FillRule as StyloFillRule;

    let svg = style.get_inherited_svg();
    let current_color = style.get_inherited_text().color;

    let fill_rule = match svg.fill_rule {
        StyloFillRule::Evenodd => Fill::EvenOdd,
        StyloFillRule::Nonzero => Fill::NonZero,
    };
    let fill_opacity = svg_opacity(&svg.fill_opacity);
    if let Some(mut color) = resolve_svg_paint(&svg.fill, &current_color) {
        color = color.multiply_alpha(fill_opacity);
        scene.fill(fill_rule, ctm, color, None, path);
    }

    let width = svg_length_px(&svg.stroke_width, viewport);
    // Stroke-width 0 skips the stroke phase entirely.
    if width > 0.0 {
        if let Some(mut color) = resolve_svg_paint(&svg.stroke, &current_color) {
            let stroke_opacity = svg_opacity(&svg.stroke_opacity);
            color = color.multiply_alpha(stroke_opacity);
            let stroke_style = Stroke::new(width);
            scene.stroke(&stroke_style, ctm, color, None, path);
        }
    }
}

fn resolve_svg_paint(
    paint: &style::values::computed::svg::SVGPaint,
    current_color: &style::color::AbsoluteColor,
) -> Option<Color> {
    use style::values::computed::svg::SVGPaintKind;
    use style::values::generics::svg::SVGPaintFallback;
    let color = match &paint.kind {
        SVGPaintKind::Color(c) => Some(c),
        SVGPaintKind::None => None,
        SVGPaintKind::PaintServer(_) => match &paint.fallback {
            SVGPaintFallback::Color(c) => Some(c),
            SVGPaintFallback::None | SVGPaintFallback::Unset => None,
        },
        SVGPaintKind::ContextFill | SVGPaintKind::ContextStroke => None,
    };
    color.map(|c| c.resolve_to_absolute(current_color).as_srgb_color())
}

fn svg_opacity(opacity: &style::values::computed::svg::SVGOpacity) -> f32 {
    use style::values::computed::svg::SVGOpacity as Opacity;
    match opacity {
        Opacity::Opacity(v) => *v,
        Opacity::ContextFillOpacity | Opacity::ContextStrokeOpacity => 1.0,
    }
}

fn svg_length_px(length: &style::values::computed::svg::SVGWidth, viewport: kurbo::Size) -> f64 {
    use style::values::computed::length::Length;
    use style::values::computed::length_percentage::Unpacked;
    use style::values::computed::svg::SVGWidth;
    let diag = blitz_dom::svg::geometry::diagonal_basis(viewport.width, viewport.height);
    match length {
        SVGWidth::LengthPercentage(lp) => match lp.0.unpack() {
            Unpacked::Length(l) => l.px() as f64,
            Unpacked::Percentage(p) => p.0 as f64 * diag,
            Unpacked::Calc(c) => c.resolve(Length::new(diag as f32)).px() as f64,
        },
        SVGWidth::ContextValue => 1.0,
    }
}

/// Resolve the SVG paint value `currentColor` against the element's computed CSS `color`.
fn resolve_current_color(style: &style::properties::ComputedValues) -> Color {
    style.get_inherited_text().color.as_srgb_color()
}

#[cfg(test)]
mod tests {
    use super::*;
    use style::color::AbsoluteColor;
    use style::values::computed::svg::{SVGOpacity, SVGPaint, SVGPaintKind};
    use style::values::generics::svg::SVGPaintFallback;

    #[test]
    fn none_paint_is_no_paint() {
        let cc = AbsoluteColor::BLACK;
        let paint = SVGPaint {
            kind: SVGPaintKind::None,
            fallback: SVGPaintFallback::Unset,
        };
        assert_eq!(resolve_svg_paint(&paint, &cc), None);
    }

    #[test]
    fn color_paint_resolves_directly() {
        use style::values::computed::color::Color as ComputedColor;
        let cc = AbsoluteColor::BLACK;
        let red = AbsoluteColor::new(style::color::ColorSpace::Srgb, 1.0, 0.0, 0.0, 1.0);
        let paint = SVGPaint {
            kind: SVGPaintKind::Color(ComputedColor::Absolute(red)),
            fallback: SVGPaintFallback::Unset,
        };
        assert_eq!(
            resolve_svg_paint(&paint, &cc),
            Some(Color::new([1.0, 0.0, 0.0, 1.0]))
        );
    }

    #[test]
    fn context_opacity_falls_back_to_opaque() {
        assert_eq!(svg_opacity(&SVGOpacity::Opacity(0.5)), 0.5);
        assert_eq!(svg_opacity(&SVGOpacity::ContextFillOpacity), 1.0);
        assert_eq!(svg_opacity(&SVGOpacity::ContextStrokeOpacity), 1.0);
    }

    #[test]
    fn percentage_stroke_width_resolves_against_the_diagonal_basis_not_zero() {
        use style::values::computed::Percentage;
        use style::values::computed::length_percentage::LengthPercentage;
        use style::values::computed::svg::SVGWidth;
        use style::values::generics::NonNegative;

        let ten_percent =
            SVGWidth::LengthPercentage(NonNegative(LengthPercentage::new_percent(Percentage(0.1))));
        let viewport = kurbo::Size::new(100.0, 100.0);
        assert!((svg_length_px(&ten_percent, viewport) - 10.0).abs() < 1e-3);
    }

    #[test]
    fn context_value_stroke_width_falls_back_to_the_svg_initial_one() {
        use style::values::computed::svg::SVGWidth;
        assert_eq!(
            svg_length_px(&SVGWidth::ContextValue, kurbo::Size::new(100.0, 100.0)),
            1.0
        );
    }

    #[test]
    fn non_finite_ctm_or_bbox_fails_the_paint_guard() {
        let nan_ctm = Affine::new([1.0, 0.0, 0.0, 1.0, f64::NAN, 0.0]);
        assert!(!nan_ctm.is_finite());

        let inf_ctm = Affine::new([f64::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0]);
        assert!(!inf_ctm.is_finite());

        let ok_ctm = Affine::IDENTITY;
        assert!(ok_ctm.is_finite());

        let nan_bbox = kurbo::Rect::new(0.0, 0.0, f64::NAN, 10.0);
        assert!(!nan_bbox.is_finite());
    }
}
