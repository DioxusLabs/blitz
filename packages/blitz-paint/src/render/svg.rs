//! Native inline SVG painting.
//!
//! A natively rendered inline `<svg>` (`svg-native` feature) keeps
//! its subtree as live DOM nodes styled by Stylo. SVG layout
//! (`blitz_dom::svg`) has already resolved each element's transform, geometry
//! and stroke; painting walks the subtree, reads those results and the paint
//! properties from computed styles. Patterns, `<use>`, `<text>`,
//! `<image>`, markers, clipping, masking and filters are not painted yet.

use anyrender::PaintScene;
use blitz_dom::node::Node;
use blitz_dom::svg::{
    SvgGradient, SvgGradientShape, SvgLayoutKind, SvgPaintServer, SvgSpreadMethod, SvgViewport,
};
use color::{ColorSpaceTag, DynamicColor};
use kurbo::{Affine, BezPath, Rect, Stroke};
use peniko::{BlendMode, ColorStop, Extend, Fill, Gradient, InterpolationAlphaSpace};
use style::computed_values::fill_rule::T as FillRule;
use style::computed_values::isolation::T as Isolation;
use style::computed_values::visibility::T as Visibility;
use style::properties::ComputedValues;
use style::values::computed::svg::{SVGOpacity, SVGPaint, SVGPaintKind};
use style::values::generics::svg::SVGPaintFallback;

use super::ElementCx;
use crate::blend::convert_blend_mode;
use crate::color::{Color, ToColorColor};

impl ElementCx<'_, '_> {
    /// Paint a natively rendered inline `<svg>` root into its content box.
    pub(super) fn draw_native_svg(&self, scene: &mut impl PaintScene) {
        let Some(root) = self.element.svg_root_data() else {
            return;
        };
        // A zero-sized viewBox disables rendering of the element.
        let Some(layout) = &root.layout else {
            return;
        };
        if root.dims.degenerate_view_box {
            return;
        }
        let content_box = self.frame.content_box();
        if content_box.width() <= 0.0 || content_box.height() <= 0.0 {
            return;
        }
        // The content box is in device pixels and the root layout maps user
        // units onto CSS pixels.
        let transform = self.transform
            * Affine::translate(content_box.origin().to_vec2())
            * Affine::scale(self.scale)
            * layout.children_transform();
        let SvgLayoutKind::Viewport { viewport, .. } = layout.kind else {
            return;
        };
        // The outermost `<svg>` is an isolated group, so blending inside it
        // does not reach content behind it.
        // https://drafts.fxtf.org/compositing/#csscompositingrules_SVG
        if layout.has_blending {
            scene.push_layer(
                Fill::NonZero,
                BlendMode::default(),
                1.0,
                Affine::IDENTITY,
                &self.viewport_rect(),
                None,
                None,
            );
        }
        self.paint_svg_children(scene, self.node, transform, viewport);
        if layout.has_blending {
            scene.pop_layer();
        }
    }

    /// The whole painted area in device pixels, for layers that must not clip.
    fn viewport_rect(&self) -> Rect {
        let painter = self.context;
        Rect::new(
            painter.initial_x,
            painter.initial_y,
            painter.initial_x + painter.width as f64,
            painter.initial_y + painter.height as f64,
        )
    }

    fn paint_svg_children(
        &self,
        scene: &mut impl PaintScene,
        node: &Node,
        transform: Affine,
        viewport: SvgViewport,
    ) {
        for child in node.children.iter() {
            self.paint_svg_node(scene, node.with(*child), transform, viewport);
        }
    }

    fn paint_svg_node(
        &self,
        scene: &mut impl PaintScene,
        node: &Node,
        parent_transform: Affine,
        viewport: SvgViewport,
    ) {
        let Some(layout) = node.element_data().and_then(|el| el.svg_layout()) else {
            return;
        };
        if matches!(layout.kind, SvgLayoutKind::NotRendered) {
            return;
        }
        let Some(style) = node.primary_styles() else {
            return;
        };
        let transform = parent_transform * layout.transform;

        // https://drafts.fxtf.org/compositing/#csscompositingrules_SVG
        let effects = style.get_effects();
        let blend = convert_blend_mode(effects.mix_blend_mode);
        let isolate = style.get_box().isolation == Isolation::Isolate;
        let grouped = effects.opacity < 1.0 || isolate || blend != BlendMode::default();
        if grouped {
            // The layer covers the whole viewport so that overflowing content
            // and strokes are not cut off; `<svg>` clipping is a separate layer.
            scene.push_layer(
                Fill::NonZero,
                blend,
                effects.opacity,
                Affine::IDENTITY,
                &self.viewport_rect(),
                None,
                None,
            );
        }

        match &layout.kind {
            SvgLayoutKind::NotRendered => {}
            SvgLayoutKind::Group => self.paint_svg_children(scene, node, transform, viewport),
            SvgLayoutKind::Switch { child } => {
                if let Some(child) = child {
                    self.paint_svg_node(scene, node.with(*child), transform, viewport);
                }
            }
            SvgLayoutKind::Viewport {
                content_transform,
                clip,
                viewport,
            } => {
                if let Some(clip) = clip {
                    scene.push_clip_layer(Fill::NonZero, transform, clip);
                }
                self.paint_svg_children(scene, node, transform * *content_transform, *viewport);
                if clip.is_some() {
                    scene.pop_layer();
                }
            }
            SvgLayoutKind::Shape { path, stroke } => {
                if style.get_inherited_box().visibility == Visibility::Visible {
                    let target = PaintTarget {
                        style: &style,
                        bbox: layout.bbox,
                        viewport,
                    };
                    self.paint_shape(scene, &target, transform, path, stroke.as_ref());
                }
            }
        }

        if grouped {
            scene.pop_layer();
        }
    }
}

/// The element an SVG paint is resolved for.
struct PaintTarget<'a> {
    style: &'a ComputedValues,
    bbox: Option<Rect>,
    viewport: SvgViewport,
}

/// A resolved fill or stroke paint: a brush and its transform relative to the
/// shape's user space.
enum ShapePaint {
    Solid(Color),
    Gradient(Gradient, Affine),
}

impl ElementCx<'_, '_> {
    /// Resolve an SVG paint, or `None` when nothing is painted.
    fn svg_paint(
        &self,
        target: &PaintTarget,
        paint: &SVGPaint,
        opacity: &SVGOpacity,
    ) -> Option<ShapePaint> {
        let style = target.style;
        let alpha = match opacity {
            SVGOpacity::Opacity(opacity) => *opacity,
            SVGOpacity::ContextFillOpacity | SVGOpacity::ContextStrokeOpacity => 1.0,
        };
        let solid = |color| {
            Some(ShapePaint::Solid(
                style
                    .resolve_color(color)
                    .as_srgb_color()
                    .multiply_alpha(alpha),
            ))
        };
        match &paint.kind {
            SVGPaintKind::None | SVGPaintKind::ContextFill | SVGPaintKind::ContextStroke => None,
            SVGPaintKind::Color(color) => solid(color),
            SVGPaintKind::PaintServer(url) => {
                match self
                    .context
                    .dom
                    .resolve_svg_paint_server(url, target.bbox, target.viewport)
                {
                    Some(SvgPaintServer::None) => None,
                    Some(SvgPaintServer::Color(color)) => Some(ShapePaint::Solid(
                        color.as_srgb_color().multiply_alpha(alpha),
                    )),
                    Some(SvgPaintServer::Gradient(gradient)) => {
                        Some(gradient_paint(&gradient, alpha))
                    }
                    // An invalid reference uses the fallback, if any.
                    None => match &paint.fallback {
                        SVGPaintFallback::Color(color) => solid(color),
                        SVGPaintFallback::None | SVGPaintFallback::Unset => None,
                    },
                }
            }
        }
    }

    fn paint_shape(
        &self,
        scene: &mut impl PaintScene,
        target: &PaintTarget,
        transform: Affine,
        path: &BezPath,
        stroke: Option<&Stroke>,
    ) {
        let svg = target.style.get_inherited_svg();
        if let Some(paint) = self.svg_paint(target, &svg.fill, &svg.fill_opacity) {
            let rule = match svg.fill_rule {
                FillRule::Nonzero => Fill::NonZero,
                FillRule::Evenodd => Fill::EvenOdd,
            };
            match &paint {
                ShapePaint::Solid(color) => scene.fill(rule, transform, *color, None, path),
                ShapePaint::Gradient(gradient, brush_transform) => {
                    scene.fill(rule, transform, gradient, Some(*brush_transform), path)
                }
            }
        }
        if let Some(stroke) = stroke
            && let Some(paint) = self.svg_paint(target, &svg.stroke, &svg.stroke_opacity)
        {
            match &paint {
                ShapePaint::Solid(color) => scene.stroke(stroke, transform, *color, None, path),
                ShapePaint::Gradient(gradient, brush_transform) => {
                    scene.stroke(stroke, transform, gradient, Some(*brush_transform), path)
                }
            }
        }
    }
}

fn gradient_paint(gradient: &SvgGradient, alpha: f32) -> ShapePaint {
    // A padded concentric gradient with a focal radius is a plain radial
    // gradient whose stops start at `fr / r`.
    let (shape, start_offset) = match gradient.shape {
        SvgGradientShape::Radial {
            focal,
            focal_radius,
            center,
            radius,
        } if focal == center
            && focal_radius > 0.0
            && focal_radius < radius
            && gradient.spread == SvgSpreadMethod::Pad =>
        {
            let shape = SvgGradientShape::Radial {
                focal,
                focal_radius: 0.0,
                center,
                radius,
            };
            (shape, (focal_radius / radius) as f32)
        }
        shape => (shape, 0.0),
    };
    let stops: Vec<ColorStop> = gradient
        .stops
        .iter()
        .map(|stop| ColorStop {
            offset: start_offset + stop.offset * (1.0 - start_offset),
            color: DynamicColor::from_alpha_color(stop.color.as_srgb_color().multiply_alpha(alpha)),
        })
        .collect();
    let mut brush = match shape {
        SvgGradientShape::Linear { start, end } => Gradient::new_linear(start, end),
        SvgGradientShape::Radial {
            focal,
            focal_radius,
            center,
            radius,
        } if focal == center && focal_radius == 0.0 => Gradient::new_radial(center, radius as f32),
        SvgGradientShape::Radial {
            focal,
            focal_radius,
            center,
            radius,
        } => Gradient::new_two_point_radial(focal, focal_radius as f32, center, radius as f32),
    }
    .with_stops(stops.as_slice())
    .with_interpolation_alpha_space(InterpolationAlphaSpace::Unpremultiplied)
    .with_extend(match gradient.spread {
        SvgSpreadMethod::Pad => Extend::Pad,
        SvgSpreadMethod::Reflect => Extend::Reflect,
        SvgSpreadMethod::Repeat => Extend::Repeat,
    });
    if gradient.linear_rgb {
        brush.interpolation_cs = ColorSpaceTag::LinearSrgb;
    }
    ShapePaint::Gradient(brush, gradient.transform)
}
