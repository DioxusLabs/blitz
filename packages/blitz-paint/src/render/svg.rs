//! Native inline SVG painting.
//!
//! A natively rendered inline `<svg>` (`svg-native` feature) keeps
//! its subtree as live DOM nodes styled by Stylo. SVG layout
//! (`blitz_dom::svg`) has already resolved each element's transform, geometry
//! and stroke; painting walks the subtree, reads those results and the paint
//! properties from computed styles. Paint servers, `<use>`, `<text>`,
//! `<image>`, markers, clipping, masking and filters are not painted yet.

use anyrender::PaintScene;
use blitz_dom::node::Node;
use blitz_dom::svg::SvgLayoutKind;
use kurbo::{Affine, BezPath, Rect, Stroke};
use peniko::{BlendMode, Fill};
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
        self.paint_svg_children(scene, self.node, transform);
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

    fn paint_svg_children(&self, scene: &mut impl PaintScene, node: &Node, transform: Affine) {
        for child in node.children.iter() {
            self.paint_svg_node(scene, node.with(*child), transform);
        }
    }

    fn paint_svg_node(&self, scene: &mut impl PaintScene, node: &Node, parent_transform: Affine) {
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
            SvgLayoutKind::Group => self.paint_svg_children(scene, node, transform),
            SvgLayoutKind::Switch { child } => {
                if let Some(child) = child {
                    self.paint_svg_node(scene, node.with(*child), transform);
                }
            }
            SvgLayoutKind::Viewport {
                content_transform,
                clip,
                ..
            } => {
                if let Some(clip) = clip {
                    scene.push_clip_layer(Fill::NonZero, transform, clip);
                }
                self.paint_svg_children(scene, node, transform * *content_transform);
                if clip.is_some() {
                    scene.pop_layer();
                }
            }
            SvgLayoutKind::Shape { path, stroke } => {
                if style.get_inherited_box().visibility == Visibility::Visible {
                    paint_shape(scene, &style, transform, path, stroke.as_ref());
                }
            }
        }

        if grouped {
            scene.pop_layer();
        }
    }
}

/// Resolve an SVG paint to a solid color, or `None` for `none`. Paint servers
/// (`url(...)`) are not supported yet and use their fallback color, if any.
fn solid_paint(style: &ComputedValues, paint: &SVGPaint, opacity: &SVGOpacity) -> Option<Color> {
    let color = match &paint.kind {
        SVGPaintKind::None | SVGPaintKind::ContextFill | SVGPaintKind::ContextStroke => {
            return None;
        }
        SVGPaintKind::Color(color) => color,
        SVGPaintKind::PaintServer(_) => match &paint.fallback {
            SVGPaintFallback::Color(color) => color,
            SVGPaintFallback::None | SVGPaintFallback::Unset => return None,
        },
    };
    let alpha = match opacity {
        SVGOpacity::Opacity(opacity) => *opacity,
        SVGOpacity::ContextFillOpacity | SVGOpacity::ContextStrokeOpacity => 1.0,
    };
    let color = style.resolve_color(color).as_srgb_color();
    Some(color.multiply_alpha(alpha))
}

fn paint_shape(
    scene: &mut impl PaintScene,
    style: &ComputedValues,
    transform: Affine,
    path: &BezPath,
    stroke: Option<&Stroke>,
) {
    let svg = style.get_inherited_svg();
    if let Some(color) = solid_paint(style, &svg.fill, &svg.fill_opacity) {
        let rule = match svg.fill_rule {
            FillRule::Nonzero => Fill::NonZero,
            FillRule::Evenodd => Fill::EvenOdd,
        };
        scene.fill(rule, transform, color, None, path);
    }
    if let Some(stroke) = stroke
        && let Some(color) = solid_paint(style, &svg.stroke, &svg.stroke_opacity)
    {
        scene.stroke(stroke, transform, color, None, path);
    }
}
