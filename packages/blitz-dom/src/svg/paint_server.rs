//! Gradient paint servers for native SVG.
//!
//! `fill`/`stroke: url(#id)` is a paint-only reference: the painter resolves
//! the server by id when it reaches a shape, so changes to a gradient, its
//! stops or its `href` chain need no dependency tracking. Stop colors come
//! from the stops' computed `stop-color`/`stop-opacity`.
//! <https://svgwg.org/svg2-draft/pservers.html>

use kurbo::{Affine, Point, Rect};
use markup5ever::{LocalName, local_name, ns};
use style::color::AbsoluteColor;
use style::computed_values::color_interpolation::T as ColorInterpolation;
use style::properties::ComputedValues;
use style::values::computed::Rotate;
use style::values::computed::url::ComputedUrl;
use style::values::generics::transform::{Scale, Translate};
use url::Url;

use super::geometry::{font_size, resolve_svg_length};
use super::{SvgViewport, attr};
use crate::node::{ElementData, Node};
use crate::{BaseDocument, resolve_2d_transform};

/// The longest `href` chain followed between gradients.
const MAX_HREF_DEPTH: usize = 32;

/// A resolved paint server, in the referencing element's user space.
#[derive(Debug, Clone)]
pub enum SvgPaintServer {
    /// Paint nothing (a gradient without stops).
    None,
    /// A solid color (a gradient with a single stop, or one whose geometry
    /// makes it paint its last stop everywhere).
    Color(AbsoluteColor),
    Gradient(SvgGradient),
}

#[derive(Debug, Clone)]
pub struct SvgGradient {
    pub shape: SvgGradientShape,
    /// Sorted by offset, at least two.
    pub stops: Vec<SvgGradientStop>,
    pub spread: SvgSpreadMethod,
    /// Maps the gradient's coordinate system (`gradientUnits`, then
    /// `gradientTransform`) onto the user space of the painted element.
    pub transform: Affine,
    /// Interpolate in linear-light sRGB (`color-interpolation: linearRGB`).
    pub linear_rgb: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum SvgGradientShape {
    Linear {
        start: Point,
        end: Point,
    },
    /// From the focal circle (`fx`, `fy`, `fr`) to the end circle
    /// (`cx`, `cy`, `r`).
    Radial {
        focal: Point,
        focal_radius: f64,
        center: Point,
        radius: f64,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct SvgGradientStop {
    pub offset: f32,
    /// `stop-color` with `stop-opacity` applied.
    pub color: AbsoluteColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgSpreadMethod {
    Pad,
    Reflect,
    Repeat,
}

fn is_gradient(element: &ElementData) -> bool {
    element.name.ns == ns!(svg)
        && (element.name.local == local_name!("linearGradient")
            || element.name.local == local_name!("radialGradient"))
}

/// `href`, which takes precedence over `xlink:href`.
fn href(element: &ElementData) -> Option<&str> {
    let attrs = element.attrs();
    let find = |namespace| {
        attrs
            .iter()
            .find(|a| a.name.local == local_name!("href") && a.name.ns == namespace)
            .map(|a| a.value.as_str())
    };
    find(ns!()).or_else(|| find(ns!(xlink)))
}

/// A displayed gradient: one with computed styles (not inside a
/// `display: none` subtree) that is not itself `display: none`.
fn is_displayed(node: &Node) -> bool {
    node.primary_styles()
        .is_some_and(|style| !style.get_box().display.is_none())
}

/// Whether a gradient specifies a transform: `gradientTransform` maps to
/// `transform`, and the individual transform properties count too.
fn specifies_transform(style: &ComputedValues) -> bool {
    let b = style.get_box();
    !b.transform.0.is_empty()
        || !matches!(b.translate, Translate::None)
        || !matches!(b.rotate, Rotate::None)
        || !matches!(b.scale, Scale::None)
}

impl BaseDocument {
    /// The id a reference to an element of this document points at, or
    /// `None` for a reference to another document.
    pub(super) fn same_document_id(&self, url: &Url) -> Option<String> {
        if !self.url.is_same_document(url) {
            return None;
        }
        let fragment = url.fragment()?;
        Some(
            percent_encoding::percent_decode_str(fragment)
                .decode_utf8_lossy()
                .into_owned(),
        )
    }

    /// Resolve a `url()` paint to a paint server for an element with object
    /// bounding box `bbox` in `viewport`. `None` when the reference is
    /// invalid and the paint's fallback applies.
    pub fn resolve_svg_paint_server(
        &self,
        url: &ComputedUrl,
        bbox: Option<Rect>,
        viewport: SvgViewport,
    ) -> Option<SvgPaintServer> {
        let id = self.same_document_id(url.url()?)?;
        let node = self.get_node(self.get_element_by_id(&id)?)?;
        if !is_gradient(node.element_data()?) {
            return None;
        }
        self.resolve_gradient(node, bbox, viewport)
    }

    /// The gradient followed by the gradients it references through `href`.
    fn gradient_chain<'a>(&'a self, start: &'a Node) -> Vec<&'a Node> {
        let mut chain = vec![start];
        let mut current = start;
        while chain.len() < MAX_HREF_DEPTH {
            let Some(next) = current
                .element_data()
                .and_then(href)
                .and_then(|href| self.url.resolve_relative(href.trim()))
                .and_then(|url| self.same_document_id(&url))
                .and_then(|id| self.get_element_by_id(&id))
                .and_then(|id| self.get_node(id))
            else {
                break;
            };
            if chain.iter().any(|n| n.id == next.id)
                || !next.element_data().is_some_and(is_gradient)
            {
                break;
            }
            chain.push(next);
            current = next;
        }
        chain
    }

    fn resolve_gradient(
        &self,
        node: &Node,
        bbox: Option<Rect>,
        viewport: SvgViewport,
    ) -> Option<SvgPaintServer> {
        let chain = self.gradient_chain(node);
        // SVG 2 keeps `display: none` gradients usable, but their stops then
        // have no computed styles, so such a chain is treated as invalid.
        if !chain.iter().all(|n| is_displayed(n)) {
            return None;
        }
        let element = node.element_data()?;
        let style = node.primary_styles()?;

        // Attributes not set on a gradient are inherited along the chain.
        // Geometry is inherited only from gradients of the same kind.
        let any_attr = |name: &str| {
            chain
                .iter()
                .filter_map(|n| attr(n.element_data()?, name))
                .find(|v| !v.trim().is_empty())
        };
        let geometry_attr = |name: &str| {
            chain
                .iter()
                .filter_map(|n| n.element_data())
                .filter(|el| el.name.local == element.name.local)
                .find_map(|el| attr(el, name))
        };

        let object_bbox = any_attr("gradientUnits").map(str::trim) != Some("userSpaceOnUse");
        let units = if object_bbox {
            let bbox = bbox.filter(|b| b.width() > 0.0 && b.height() > 0.0)?;
            Affine::new([bbox.width(), 0.0, 0.0, bbox.height(), bbox.x0, bbox.y0])
        } else {
            Affine::IDENTITY
        };
        let gradient_transform = chain
            .iter()
            .filter_map(|n| n.primary_styles())
            .find(|style| specifies_transform(style))
            .and_then(|style| resolve_2d_transform(style.get_box(), euclid::default::Rect::zero()))
            .unwrap_or_default();
        // A non-invertible transform makes the reference invalid.
        let det = gradient_transform.determinant();
        if det == 0.0 || !det.is_finite() {
            return None;
        }

        // The stops of the first gradient in the chain that has any.
        let stops = chain
            .iter()
            .map(|n| self.gradient_stops(n))
            .find(|stops| !stops.is_empty())
            .unwrap_or_default();
        match stops.as_slice() {
            [] => return Some(SvgPaintServer::None),
            [stop] => return Some(SvgPaintServer::Color(stop.color)),
            _ => {}
        }
        let spread = match any_attr("spreadMethod").map(str::trim) {
            Some("reflect") => SvgSpreadMethod::Reflect,
            Some("repeat") => SvgSpreadMethod::Repeat,
            _ => SvgSpreadMethod::Pad,
        };

        let viewport = viewport.for_element(&style);
        let font_size = viewport.unzoom(font_size(&style));
        let length = |name: &str, basis: f64| {
            let len: svgtypes::Length = geometry_attr(name)?.trim().parse().ok()?;
            Some(if object_bbox {
                // Fractions of the bounding box; percentages are of 1.
                resolve_svg_length(len, 1.0, font_size)
            } else {
                resolve_svg_length(len, basis, font_size)
            })
        };
        let (w, h, d) = (viewport.width, viewport.height, viewport.diagonal());
        let half = |basis: f64| if object_bbox { 0.5 } else { basis / 2.0 };

        let last_color = stops.last().unwrap().color;
        let shape = if element.name.local == local_name!("linearGradient") {
            let start = Point::new(
                length("x1", w).unwrap_or(0.0),
                length("y1", h).unwrap_or(0.0),
            );
            let end = Point::new(
                length("x2", w).unwrap_or(if object_bbox { 1.0 } else { w }),
                length("y2", h).unwrap_or(0.0),
            );
            // A zero-length vector paints the last stop.
            if start == end {
                return Some(SvgPaintServer::Color(last_color));
            }
            SvgGradientShape::Linear { start, end }
        } else {
            let center = Point::new(
                length("cx", w).unwrap_or(half(w)),
                length("cy", h).unwrap_or(half(h)),
            );
            let radius = length("r", d).unwrap_or(half(d));
            let focal = Point::new(
                length("fx", w).unwrap_or(center.x),
                length("fy", h).unwrap_or(center.y),
            );
            let focal_radius = length("fr", d).unwrap_or(0.0).max(0.0);
            // A zero (or negative, which is an error) radius paints the last
            // stop.
            if radius <= 0.0 {
                return Some(SvgPaintServer::Color(last_color));
            }
            SvgGradientShape::Radial {
                focal,
                focal_radius,
                center,
                radius,
            }
        };

        Some(SvgPaintServer::Gradient(SvgGradient {
            shape,
            stops,
            spread,
            transform: units * gradient_transform,
            linear_rgb: style.get_inherited_svg().color_interpolation
                == ColorInterpolation::Linearrgb,
        }))
    }

    /// The `<stop>` children of a gradient, with offsets clamped to be
    /// non-decreasing. <https://svgwg.org/svg2-draft/pservers.html#StopNotes>
    fn gradient_stops(&self, gradient: &Node) -> Vec<SvgGradientStop> {
        let stop_tag: LocalName = local_name!("stop");
        let mut stops = Vec::new();
        let mut last_offset = 0.0f32;
        for child in gradient.children.iter().filter_map(|id| self.get_node(*id)) {
            let Some(element) = child.element_data() else {
                continue;
            };
            if element.name.ns != ns!(svg) || element.name.local != stop_tag {
                continue;
            }
            let offset = attr(element, "offset")
                .and_then(|v| v.trim().parse::<svgtypes::Length>().ok())
                .map_or(0.0, |len| match len.unit {
                    svgtypes::LengthUnit::None => len.number,
                    svgtypes::LengthUnit::Percent => len.number / 100.0,
                    // Only numbers and percentages are valid.
                    _ => 0.0,
                }) as f32;
            let offset = offset.clamp(0.0, 1.0).max(last_offset);
            last_offset = offset;
            // Stops of a displayed gradient always have computed styles.
            let Some(style) = child.primary_styles() else {
                continue;
            };
            let svg = style.get_svg();
            let mut color = style.resolve_color(&svg.stop_color);
            color.alpha *= svg.stop_opacity.clamp(0.0, 1.0);
            stops.push(SvgGradientStop { offset, color });
        }
        stops
    }
}
