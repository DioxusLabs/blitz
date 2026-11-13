//! `filter` references to `<filter>` elements.
//!
//! Like clip paths and masks these are paint-only references, resolved by
//! the painter for each `url(#id)` in an element's `filter` list. The
//! `feFlood`, `feGaussianBlur`, `feOffset`, `feDropShadow` and `feBlend`
//! primitives are supported. A filter with any other primitive is ignored
//! and the element painted unfiltered.
//! <https://drafts.fxtf.org/filter-effects-1/>

use std::collections::HashMap;

use kurbo::Rect;
use markup5ever::{local_name, ns};
use style::color::AbsoluteColor;
use style::computed_values::mix_blend_mode::T as MixBlendMode;
use style::values::computed::Filter as StyloFilter;

use super::clip_mask::{SvgReference, bbox_transform, resource_region};
use super::geometry::font_size;
use super::{SvgViewport, attr};
use crate::BaseDocument;
use crate::node::Node;

/// A resolved `<filter>`.
#[derive(Debug, Clone)]
pub struct SvgFilter {
    /// The filter region in the referencing element's user space.
    pub rect: Rect,
    /// The primitives in document order; the last one's result is the
    /// filter's output. Lengths are in the referencing element's user space.
    pub primitives: Vec<SvgFilterPrimitive>,
}

#[derive(Debug, Clone)]
pub struct SvgFilterPrimitive {
    pub kind: SvgFilterKind,
    /// `in`, with the default (the previous result, or `SourceGraphic` for
    /// the first primitive) resolved.
    pub input: SvgFilterInput,
    /// `in2` of `feBlend`.
    pub input2: SvgFilterInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgFilterInput {
    SourceGraphic,
    SourceAlpha,
    /// The result of an earlier primitive, by index.
    Result(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SvgFilterKind {
    /// `feFlood`: `flood-color` with `flood-opacity` applied.
    Flood(AbsoluteColor),
    /// `feGaussianBlur`. A standard deviation of zero passes the input
    /// through.
    GaussianBlur { std_deviation: f64 },
    /// `feOffset`.
    Offset { dx: f64, dy: f64 },
    /// `feDropShadow`.
    DropShadow {
        dx: f64,
        dy: f64,
        std_deviation: f64,
        color: AbsoluteColor,
    },
    /// `feBlend`: `input` blended onto `input2`.
    Blend(MixBlendMode),
}

/// How many `href`s are followed from a `<filter>`.
const MAX_HREF_DEPTH: usize = 16;

fn parse_number(value: Option<&str>, default: f64) -> f64 {
    value
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|n| n.is_finite())
        .unwrap_or(default)
}

/// `stdDeviation`: one or two non-negative numbers. Anything else, or two
/// different numbers (which no backend supports yet), is `None`.
fn parse_std_deviation(value: Option<&str>, default: f64) -> Option<(f64, f64)> {
    let Some(value) = value else {
        return Some((default, default));
    };
    let mut numbers = value
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f64>().ok().filter(|n| n.is_finite()));
    let x = numbers.next().flatten()?;
    let y = match numbers.next() {
        Some(y) => y?,
        None => x,
    };
    if numbers.next().is_some() {
        return None;
    }
    Some((x, y))
}

fn parse_blend_mode(value: Option<&str>) -> MixBlendMode {
    match value.map(str::trim).unwrap_or("normal") {
        "normal" => MixBlendMode::Normal,
        "multiply" => MixBlendMode::Multiply,
        "screen" => MixBlendMode::Screen,
        "overlay" => MixBlendMode::Overlay,
        "darken" => MixBlendMode::Darken,
        "lighten" => MixBlendMode::Lighten,
        "color-dodge" => MixBlendMode::ColorDodge,
        "color-burn" => MixBlendMode::ColorBurn,
        "hard-light" => MixBlendMode::HardLight,
        "soft-light" => MixBlendMode::SoftLight,
        "difference" => MixBlendMode::Difference,
        "exclusion" => MixBlendMode::Exclusion,
        "hue" => MixBlendMode::Hue,
        "saturation" => MixBlendMode::Saturation,
        "color" => MixBlendMode::Color,
        "luminosity" => MixBlendMode::Luminosity,
        // An invalid mode is the initial value.
        _ => MixBlendMode::Normal,
    }
}

/// `flood-color` with `flood-opacity` applied, from the primitive's style.
fn flood_color(node: &Node) -> AbsoluteColor {
    let Some(style) = node.svg_style() else {
        return AbsoluteColor::BLACK;
    };
    let svg = style.get_svg();
    let mut color = style.resolve_color(&svg.flood_color);
    color.alpha *= svg.flood_opacity.clamp(0.0, 1.0);
    color
}

impl BaseDocument {
    /// The `<filter>` element `href` (or `xlink:href`) points at.
    fn svg_filter_href(&self, node: &Node) -> Option<&Node> {
        let element = node.element_data()?;
        let href = attr(element, "href").or_else(|| attr(element, "xlink:href"))?;
        let id = href.trim().strip_prefix('#')?;
        let target = self.get_node(self.get_element_by_id(id)?)?;
        let element = target.element_data()?;
        (element.name.ns == ns!(svg) && element.name.local == local_name!("filter"))
            .then_some(target)
    }

    /// Resolve the `filter` of an element with computed style `style`,
    /// object bounding box `bbox` and viewport `viewport`.
    ///
    /// A reference to a missing element or something other than a `<filter>`
    /// is ignored. An invalid region or a filter without primitives disables
    /// rendering of the element.
    pub fn resolve_svg_filter(
        &self,
        filter: &StyloFilter,
        bbox: Option<Rect>,
        viewport: SvgViewport,
    ) -> SvgReference<SvgFilter> {
        let StyloFilter::Url(url) = filter else {
            return SvgReference::None;
        };
        let Some(node) = url
            .url()
            .and_then(|url| self.same_document_id(url))
            .and_then(|id| self.get_element_by_id(&id))
            .and_then(|id| self.get_node(id))
            .filter(|node| {
                node.element_data().is_some_and(|el| {
                    el.name.ns == ns!(svg) && el.name.local == local_name!("filter")
                })
            })
        else {
            return SvgReference::None;
        };

        // The filter and the filters it inherits attributes and primitives
        // from through `href`.
        let mut chain = vec![node];
        while chain.len() < MAX_HREF_DEPTH
            && let Some(next) = self.svg_filter_href(chain[chain.len() - 1])
            && !chain.iter().any(|n| n.id == next.id)
        {
            chain.push(next);
        }
        let get = |name: &str| {
            chain
                .iter()
                .find_map(|n| attr(n.element_data()?, name))
                .map(str::trim)
        };

        let font_size = node.svg_style().map_or(16.0, |style| font_size(&style));
        let user_space = get("filterUnits") == Some("userSpaceOnUse");
        let Some(rect) = resource_region(get, user_space, bbox, viewport, font_size) else {
            return SvgReference::Invalid;
        };

        // `primitiveUnits="objectBoundingBox"` makes lengths fractions of
        // the bounding box.
        let (scale_x, scale_y) = if get("primitiveUnits") == Some("objectBoundingBox") {
            let Some(bbox) = bbox_transform(bbox) else {
                return SvgReference::Invalid;
            };
            let [sx, _, _, sy, _, _] = bbox.as_coeffs();
            (sx, sy)
        } else {
            (1.0, 1.0)
        };

        let is_primitive = |node: &&Node| {
            node.element_data()
                .is_some_and(|el| el.name.ns == ns!(svg) && el.name.local.starts_with("fe"))
        };
        let Some(content) = chain.iter().find(|filter| {
            filter
                .children
                .iter()
                .any(|&id| is_primitive(&&self.nodes[id]))
        }) else {
            return SvgReference::Invalid;
        };

        let mut primitives: Vec<SvgFilterPrimitive> = Vec::new();
        let mut results: HashMap<&str, usize> = HashMap::new();
        for child in content.children.iter().map(|&id| &self.nodes[id]) {
            if !is_primitive(&child) {
                continue;
            }
            let element = child.element_data().unwrap();
            let get = |name: &str| attr(element, name).map(str::trim);
            let default_input = match primitives.len() {
                0 => SvgFilterInput::SourceGraphic,
                n => SvgFilterInput::Result(n - 1),
            };
            let input = |name: &str| match get(name) {
                Some("SourceGraphic") => SvgFilterInput::SourceGraphic,
                Some("SourceAlpha") => SvgFilterInput::SourceAlpha,
                // Like usvg, the unsupported background and paint inputs are
                // the source graphic.
                Some("BackgroundImage" | "BackgroundAlpha" | "FillPaint" | "StrokePaint") => {
                    SvgFilterInput::SourceGraphic
                }
                Some(name) => results
                    .get(name)
                    .map_or(default_input, |&i| SvgFilterInput::Result(i)),
                None => default_input,
            };
            let offset = |default: f64| {
                (
                    parse_number(get("dx"), default) * scale_x,
                    parse_number(get("dy"), default) * scale_y,
                )
            };
            let std_deviation = |default: f64| {
                let (x, y) = parse_std_deviation(get("stdDeviation"), default)?;
                let (x, y) = (x * scale_x, y * scale_y);
                // A negative value disables the effect, like zero.
                let (x, y) = (x.max(0.0), y.max(0.0));
                ((x - y).abs() <= 1e-9 * x.max(y)).then_some(x)
            };

            let kind = match &*element.name.local {
                "feFlood" => SvgFilterKind::Flood(flood_color(child)),
                "feGaussianBlur" => {
                    let Some(std_deviation) = std_deviation(0.0) else {
                        return SvgReference::None;
                    };
                    SvgFilterKind::GaussianBlur { std_deviation }
                }
                "feOffset" => {
                    let (dx, dy) = offset(0.0);
                    SvgFilterKind::Offset { dx, dy }
                }
                "feDropShadow" => {
                    let Some(std_deviation) = std_deviation(2.0) else {
                        return SvgReference::None;
                    };
                    let (dx, dy) = offset(2.0);
                    SvgFilterKind::DropShadow {
                        dx,
                        dy,
                        std_deviation,
                        color: flood_color(child),
                    }
                }
                "feBlend" => SvgFilterKind::Blend(parse_blend_mode(get("mode"))),
                _ => return SvgReference::None,
            };
            let input2 = match kind {
                SvgFilterKind::Blend(_) => input("in2"),
                _ => SvgFilterInput::SourceGraphic,
            };
            primitives.push(SvgFilterPrimitive {
                input: input("in"),
                input2,
                kind,
            });
            if let Some(result) = get("result").filter(|r| !r.is_empty()) {
                results.insert(result, primitives.len() - 1);
            }
        }

        SvgReference::Valid(SvgFilter { rect, primitives })
    }
}
