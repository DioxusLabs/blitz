//! Native inline SVG painting.
//!
//! A natively rendered inline `<svg>` (`svg-native` feature, picked by
//! `DocumentConfig::native_svg` when usvg is also compiled in) keeps
//! its subtree as live DOM nodes styled by Stylo, so painting walks those
//! nodes and reads everything (geometry, paint, transforms) from their
//! computed styles. This is the foundation only: basic shapes with solid
//! fills and strokes, `<g>`, nested `<svg>` and `transform`. Paint servers,
//! `<use>`, `<text>`, `<image>`, markers, clipping, masking and filters are
//! not painted yet.

use anyrender::PaintScene;
use blitz_dom::node::Node;
use blitz_dom::{CopyStyleGetters, local_name, resolve_2d_transform};
use kurbo::{Affine, BezPath, Circle, Ellipse, Line, Point, Rect, RoundedRect, Shape, Stroke};
use peniko::Fill;
use style::computed_values::fill_rule::T as FillRule;
use style::computed_values::stroke_linecap::T as StrokeLinecap;
use style::computed_values::stroke_linejoin::T as StrokeLinejoin;
use style::properties::ComputedValues;
use style::values::CSSFloat;
use style::values::computed::svg::{
    SVGLength, SVGOpacity, SVGPaint, SVGPaintKind, SVGStrokeDashArray, SVGWidth,
};
use style::values::computed::{LengthPercentage, NonNegativeLengthPercentageOrAuto, Size};
use style::values::generics::svg::SVGPaintFallback;
use style::values::specified::svg::DProperty;

use super::ElementCx;
use super::clip_path::svg_path_to_bezpath;
use crate::color::{Color, ToColorColor};

/// The SVG viewport a shape's percentages resolve against, in user units.
#[derive(Clone, Copy)]
struct Viewport {
    width: f64,
    height: f64,
}

impl Viewport {
    /// <https://svgwg.org/svg2-draft/coords.html#Units>: the normalized diagonal.
    fn diagonal(&self) -> f64 {
        ((self.width * self.width + self.height * self.height) / 2.0).sqrt()
    }
    fn x(&self, lp: &LengthPercentage) -> f64 {
        resolve(lp, self.width)
    }
    fn y(&self, lp: &LengthPercentage) -> f64 {
        resolve(lp, self.height)
    }
    fn other(&self, lp: &LengthPercentage) -> f64 {
        resolve(lp, self.diagonal())
    }
}

fn resolve(lp: &LengthPercentage, basis: f64) -> f64 {
    lp.resolve(style::values::computed::Length::new(basis as f32))
        .px() as f64
}

/// Convert an SVG transform into a kurbo one.
fn to_affine(t: blitz_dom::node::SvgTransform) -> Affine {
    Affine::new([t.sx, t.ky, t.kx, t.sy, t.tx, t.ty].map(f64::from))
}

impl ElementCx<'_, '_> {
    /// Paint a natively rendered inline `<svg>` root into its content box.
    pub(super) fn draw_native_svg(&self, scene: &mut impl PaintScene) {
        let Some(dims) = self.element.svg_root_data() else {
            return;
        };
        // A zero-sized viewBox disables rendering of the element.
        if dims.degenerate_view_box {
            return;
        }
        let content_box = self.frame.content_box();
        let (width, height) = (content_box.width() as f32, content_box.height() as f32);
        if width <= 0.0 || height <= 0.0 {
            return;
        }

        // The content box is in device pixels; map user units onto it.
        let (viewport, to_viewport) = match dims.view_box {
            Some(vb) => (
                Viewport {
                    width: vb.width as f64,
                    height: vb.height as f64,
                },
                to_affine(dims.viewport_transform(width, height)),
            ),
            None => (
                Viewport {
                    width: width as f64 / self.scale,
                    height: height as f64 / self.scale,
                },
                Affine::scale(self.scale),
            ),
        };
        let transform =
            self.transform * Affine::translate(content_box.origin().to_vec2()) * to_viewport;

        for child in self.node.children.iter() {
            self.paint_svg_node(scene, self.node.with(*child), transform, viewport);
        }
    }

    fn paint_svg_node(
        &self,
        scene: &mut impl PaintScene,
        node: &Node,
        parent_transform: Affine,
        viewport: Viewport,
    ) {
        let Some(element) = node.element_data() else {
            return;
        };
        if element.name.ns != markup5ever::ns!(svg) {
            return;
        }
        let Some(style) = node.primary_styles() else {
            return;
        };
        if style.clone_display().is_none() {
            return;
        }

        // SVG content transforms about the user-space origin (UA stylesheet
        // sets `transform-origin: 0 0`), so the reference box is empty.
        let transform = parent_transform
            * resolve_2d_transform(style.get_box(), euclid::default::Rect::zero())
                .unwrap_or_default();

        let opacity = style.get_effects().opacity;
        let grouped = opacity < 1.0;
        if grouped {
            // The root clips to its box unless its overflow is visible, in
            // which case the layer must not cut anything off.
            let root_clips =
                self.style.get_box().overflow_x != style::computed_values::overflow_x::T::Visible;
            let bounds = if root_clips {
                self.frame.padding_box()
            } else {
                Rect::new(-1e9, -1e9, 1e9, 1e9)
            };
            scene.push_layer(
                Fill::NonZero,
                peniko::Mix::Normal,
                opacity,
                self.transform,
                &bounds,
                None,
                None,
            );
        }

        let visible =
            style.get_inherited_box().visibility == style::computed_values::visibility::T::Visible;
        let tag = &element.name.local;
        if is_resource_container(tag) {
            // Rendered only where referenced (not yet supported).
        } else if *tag == local_name!("g") || *tag == local_name!("a") {
            for child in node.children.iter() {
                self.paint_svg_node(scene, node.with(*child), transform, viewport);
            }
        } else if *tag == local_name!("switch") {
            // Conditional processing attributes are not evaluated yet, so the
            // first child element is the one rendered.
            if let Some(child) = node
                .children
                .iter()
                .find(|id| node.with(**id).element_data().is_some())
            {
                self.paint_svg_node(scene, node.with(*child), transform, viewport);
            }
        } else if *tag == local_name!("svg") {
            self.paint_nested_svg(scene, node, &style, transform, viewport);
        } else if visible && let Some(shape) = shape_geometry(tag, element, &style, viewport) {
            paint_shape(scene, &style, transform, viewport, &shape);
        }

        if grouped {
            scene.pop_layer();
        }
    }

    /// A nested `<svg>` establishes a new viewport at its `x`/`y`, sized by
    /// its `width`/`height` (defaulting to 100%), with its own `viewBox`.
    fn paint_nested_svg(
        &self,
        scene: &mut impl PaintScene,
        node: &Node,
        style: &ComputedValues,
        transform: Affine,
        viewport: Viewport,
    ) {
        let Some(element) = node.element_data() else {
            return;
        };
        let x = viewport.x(&style.get_svg().x);
        let y = viewport.y(&style.get_svg().y);
        let size = |size: &Size, basis: f64| match size {
            Size::LengthPercentage(lp) => resolve(&lp.0, basis),
            _ => basis,
        };
        let width = size(&style.get_position().width, viewport.width);
        let height = size(&style.get_position().height, viewport.height);
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let dims = blitz_dom::node::SvgIntrinsicDimensions::from_attrs(|name| {
            element
                .attrs()
                .iter()
                .find(|a| &*a.name.local == name)
                .map(|a| a.value.as_str())
        });
        if dims.degenerate_view_box {
            return;
        }
        let inner_viewport = match dims.view_box {
            Some(vb) => Viewport {
                width: vb.width as f64,
                height: vb.height as f64,
            },
            None => Viewport { width, height },
        };
        // The viewport rect is clipped in the outer coordinate system.
        let clips = style.get_box().overflow_x != style::computed_values::overflow_x::T::Visible;
        if clips {
            scene.push_clip_layer(
                Fill::NonZero,
                transform,
                &Rect::new(x, y, x + width, y + height),
            );
        }
        let inner_transform = transform
            * Affine::translate((x, y))
            * to_affine(dims.viewport_transform(width as f32, height as f32));
        for child in node.children.iter() {
            self.paint_svg_node(scene, node.with(*child), inner_transform, inner_viewport);
        }
        if clips {
            scene.pop_layer();
        }
    }
}

/// Elements whose content is never rendered directly, only through a
/// reference (`<use>`, `fill="url(#id)"`, `clip-path`, ...). They are kept
/// styled (not `display: none`) so that their children have computed styles.
fn is_resource_container(tag: &markup5ever::LocalName) -> bool {
    *tag == local_name!("defs")
        || *tag == local_name!("clipPath")
        || *tag == local_name!("mask")
        || *tag == local_name!("marker")
        || *tag == local_name!("pattern")
        || *tag == local_name!("symbol")
        || *tag == local_name!("filter")
        || *tag == local_name!("linearGradient")
        || *tag == local_name!("radialGradient")
}

/// The geometry of a basic shape element in user units, from its computed
/// style (`x`, `y`, `width`, `height`, `cx`, `cy`, `r`, `rx`, `ry`, `d`) or,
/// for `<line>`, `<polyline>` and `<polygon>`, whose geometry is not CSS, its
/// attributes. `None` when the element is not a shape or renders nothing.
fn shape_geometry(
    tag: &markup5ever::LocalName,
    element: &blitz_dom::ElementData,
    style: &ComputedValues,
    viewport: Viewport,
) -> Option<BezPath> {
    let svg = style.get_svg();
    let radius = |r: &NonNegativeLengthPercentageOrAuto, basis: f64| match r {
        NonNegativeLengthPercentageOrAuto::LengthPercentage(lp) => Some(resolve(&lp.0, basis)),
        NonNegativeLengthPercentageOrAuto::Auto => None,
    };
    // A raw SVG length attribute (`x1="10%"`, `y2="1em"`): percentages
    // resolve against the viewport dimension, font units against the
    // element's font size.
    let font_size = style.get_font().font_size.used_size().px() as f64;
    let attr_length = |name: &str, basis: f64| -> Option<f64> {
        use svgtypes::LengthUnit;
        let len = element
            .attrs()
            .iter()
            .find(|a| &*a.name.local == name)
            .and_then(|a| a.value.trim().parse::<svgtypes::Length>().ok())?;
        let n = len.number;
        Some(match len.unit {
            LengthUnit::None | LengthUnit::Px => n,
            LengthUnit::Percent => basis * n / 100.0,
            LengthUnit::Em => n * font_size,
            LengthUnit::Ex => n * font_size / 2.0,
            LengthUnit::In => n * 96.0,
            LengthUnit::Cm => n * 96.0 / 2.54,
            LengthUnit::Mm => n * 96.0 / 25.4,
            LengthUnit::Pt => n * 96.0 / 72.0,
            LengthUnit::Pc => n * 16.0,
        })
    };

    if *tag == local_name!("rect") {
        let size = |size: &Size, basis: f64| match size {
            Size::LengthPercentage(lp) => resolve(&lp.0, basis),
            _ => 0.0,
        };
        let width = size(&style.get_position().width, viewport.width);
        let height = size(&style.get_position().height, viewport.height);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let x = viewport.x(&svg.x);
        let y = viewport.y(&svg.y);
        let rx = radius(&svg.rx, viewport.width);
        let ry = radius(&svg.ry, viewport.height);
        let (rx, ry) = match (rx, ry) {
            (Some(rx), Some(ry)) => (rx, ry),
            (Some(r), None) | (None, Some(r)) => (r, r),
            (None, None) => (0.0, 0.0),
        };
        let rect = Rect::new(x, y, x + width, y + height);
        if rx > 0.0 && ry > 0.0 {
            // kurbo only has uniform corner radii; approximate elliptical
            // corners by scaling the rect so they become circular.
            let rx = rx.min(width / 2.0);
            let ry = ry.min(height / 2.0);
            let scale_y = ry / rx;
            let scaled = Rect::new(x, y / scale_y, x + width, (y + height) / scale_y);
            let mut path = RoundedRect::from_rect(scaled, rx).to_path(0.1);
            path.apply_affine(Affine::scale_non_uniform(1.0, scale_y));
            return Some(path);
        }
        return Some(rect.to_path(0.1));
    }
    if *tag == local_name!("circle") {
        let r = viewport.other(&svg.r.0);
        if r <= 0.0 {
            return None;
        }
        let center = Point::new(viewport.x(&svg.cx), viewport.y(&svg.cy));
        return Some(Circle::new(center, r).to_path(0.1));
    }
    if *tag == local_name!("ellipse") {
        let rx = radius(&svg.rx, viewport.width);
        let ry = radius(&svg.ry, viewport.height);
        let (rx, ry) = match (rx, ry) {
            (Some(rx), Some(ry)) => (rx, ry),
            (Some(r), None) | (None, Some(r)) => (r, r),
            (None, None) => return None,
        };
        if rx <= 0.0 || ry <= 0.0 {
            return None;
        }
        let center = Point::new(viewport.x(&svg.cx), viewport.y(&svg.cy));
        return Some(Ellipse::new(center, (rx, ry), 0.0).to_path(0.1));
    }
    if *tag == local_name!("line") {
        let p0 = Point::new(
            attr_length("x1", viewport.width).unwrap_or(0.0),
            attr_length("y1", viewport.height).unwrap_or(0.0),
        );
        let p1 = Point::new(
            attr_length("x2", viewport.width).unwrap_or(0.0),
            attr_length("y2", viewport.height).unwrap_or(0.0),
        );
        return Some(Line::new(p0, p1).to_path(0.1));
    }
    if *tag == local_name!("polyline") || *tag == local_name!("polygon") {
        let points = element.attr(local_name!("points"))?;
        let mut path = BezPath::new();
        for (i, (x, y)) in svgtypes::PointsParser::from(points).enumerate() {
            let p = Point::new(x, y);
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
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
    viewport: Viewport,
    shape: &BezPath,
) {
    let svg = style.get_inherited_svg();

    if let Some(color) = solid_paint(style, &svg.fill, &svg.fill_opacity) {
        let rule = match svg.fill_rule {
            FillRule::Nonzero => Fill::NonZero,
            FillRule::Evenodd => Fill::EvenOdd,
        };
        scene.fill(rule, transform, color, None, shape);
    }

    if let Some(color) = solid_paint(style, &svg.stroke, &svg.stroke_opacity) {
        let width = match &svg.stroke_width {
            SVGWidth::LengthPercentage(lp) => viewport.other(&lp.0),
            SVGWidth::ContextValue => 1.0,
        };
        if width <= 0.0 {
            return;
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
        if let SVGStrokeDashArray::Values(values) = &svg.stroke_dasharray {
            let dashes: Vec<f64> = values.iter().map(|lp| viewport.other(&lp.0)).collect();
            if !dashes.is_empty()
                && dashes.iter().all(|d| d.is_finite() && *d >= 0.0)
                && dashes.iter().sum::<f64>() > 0.0
            {
                let offset = match &svg.stroke_dashoffset {
                    SVGLength::LengthPercentage(lp) => viewport.other(lp),
                    SVGLength::ContextValue => 0.0,
                };
                stroke = stroke.with_dashes(offset, dashes);
            }
        }
        scene.stroke(&stroke, transform, color, None, shape);
    }
}
