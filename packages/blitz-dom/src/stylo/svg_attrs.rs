//! SVG presentation attributes → CSS declarations.
//!
//! Per SVG 2 (<https://svgwg.org/svg2-draft/styling.html#PresentationAttributes>)
//! a presentation attribute shares its name and value grammar with the CSS
//! property of the same name, so rather than hand-parsing each one the
//! attribute name is resolved to a `PropertyId` and the value is fed through
//! Stylo's own `PropertyDeclaration::parse_into`, the parser used for the
//! `style` attribute. Attribute-only grammars are handled first: `transform`
//! (an SVG transform list) becomes a CSS `matrix()` and `d` (raw path data) is
//! parsed with `SVGPathData::parse_bytes`, which keeps the path up to the
//! first error as SVG requires. Anything else that fails to parse is dropped,
//! matching how browsers treat an invalid presentation attribute.
//!
//! The geometry properties (`x`, `y`, `width`, `height`, `cx`, `cy`, `r`,
//! `rx`, `ry`, `d`) are presentation attributes only on the elements whose
//! geometry they define; elsewhere (`x` on `<text>`, `width` on `<mask>`)
//! the attribute has a different grammar and stays a plain attribute.
//!
//! Attribute values are parsed in SVG's "unitless length" mode, where
//! `stroke-width="2"` means `2px`, and the resulting declarations are
//! cascaded at the presentational-hint level: below any author CSS, as the
//! spec requires.

use cssparser::Parser;
use selectors::matching::QuirksMode;
use style::parser::ParserContext;
use style::properties::{PropertyDeclaration, PropertyId, SourcePropertyDeclaration};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::values::specified::svg::DProperty;
use style::values::specified::svg_path::SVGPathData;
use style_traits::ParsingMode;

/// Parse one SVG presentation attribute on element `tag` into CSS
/// declarations (`text-decoration` is a shorthand and expands into several), pushed
/// onto `out`, a stack-allocated buffer the caller reuses across attributes.
/// Nothing is pushed when `name` is not a presentation attribute for this
/// element or `value` does not parse as that property's value.
pub(crate) fn svg_presentation_hint(
    tag: &str,
    name: &str,
    value: &str,
    url_extra_data: &UrlExtraData,
    out: &mut SourcePropertyDeclaration,
) {
    if !is_presentation_attribute(tag, name) {
        return;
    }
    let translated;
    let value = match name {
        "transform" => {
            let Some(css) = svg_transform_to_css(value) else {
                return;
            };
            translated = css;
            translated.as_str()
        }
        "d" => {
            let (path, _ok) = SVGPathData::parse_bytes(value.as_bytes());
            out.push(PropertyDeclaration::D(DProperty::Path(path)));
            return;
        }
        _ => value,
    };

    let mut context = ParserContext::new(
        Origin::Author,
        url_extra_data,
        Some(CssRuleType::Style),
        ParsingMode::ALLOW_UNITLESS_LENGTH,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        Default::default(),
    );

    let Ok(property_id) = PropertyId::parse(name, &context) else {
        return;
    };
    let mut parser = Parser::new(value);
    if PropertyDeclaration::parse_into(out, property_id, &mut context, &mut parser).is_err() {
        out.declarations.clear();
    }
}

/// <https://svgwg.org/svg2-draft/styling.html#PresentationAttributes>: the
/// geometry properties apply only on the elements they define the geometry
/// of; every other CSS property is a presentation attribute on all elements.
fn is_presentation_attribute(tag: &str, name: &str) -> bool {
    match name {
        // https://svgwg.org/svg2-draft/styling.html#PresentationAttributes
        // Only this list maps to CSS; any other attribute that happens to
        // name a CSS property (`padding`, `background`, `rotate` on <text>,
        // `scale` on <feDisplacementMap>) stays a plain attribute. The spec's
        // other presentation attributes (`clip-rule`, `stop-color`,
        // `marker-*`, `text-anchor`, ...) name properties Stylo only parses
        // for Gecko, so they are left out until Servo's Stylo supports them.
        "alignment-baseline" | "baseline-shift" | "clip-path" | "color" | "cursor"
        | "direction" | "display" | "fill-opacity" | "fill-rule" | "filter" | "font-family"
        | "font-size" | "font-size-adjust" | "font-stretch" | "font-style" | "font-variant"
        | "font-weight" | "image-rendering" | "letter-spacing" | "mask" | "mask-type"
        | "opacity" | "overflow" | "pointer-events" | "stroke" | "stroke-dasharray"
        | "stroke-dashoffset" | "stroke-linecap" | "stroke-linejoin" | "stroke-miterlimit"
        | "stroke-opacity" | "stroke-width" | "text-decoration" | "text-overflow"
        | "text-rendering" | "transform-origin" | "unicode-bidi" | "visibility" | "white-space"
        | "word-spacing" | "writing-mode" => true,
        // On animation elements `fill` is the animation fill mode.
        "fill" => !matches!(
            tag,
            "animate" | "animateMotion" | "animateTransform" | "set"
        ),
        // Patterns and gradients spell it `patternTransform` / `gradientTransform`.
        "transform" => !matches!(tag, "pattern" | "linearGradient" | "radialGradient"),
        "cx" | "cy" => matches!(tag, "circle" | "ellipse"),
        "r" => tag == "circle",
        "rx" | "ry" => matches!(tag, "ellipse" | "rect"),
        "x" | "y" | "width" | "height" => {
            matches!(tag, "rect" | "image" | "foreignObject" | "svg")
        }
        "d" => tag == "path",
        _ => false,
    }
}

/// Translate an SVG `transform` attribute (a transform list in the SVG
/// grammar, e.g. `translate(10 20) rotate(45)`) into a CSS `matrix()`.
///
/// The two grammars differ (SVG allows unitless angles and whitespace-
/// separated arguments, and `rotate(a cx cy)` has no CSS equivalent), so the
/// list is composed into a single matrix rather than translated term by term.
pub(crate) fn svg_transform_to_css(value: &str) -> Option<String> {
    let ts: svgtypes::Transform = value.parse().ok()?;
    Some(format!(
        "matrix({}, {}, {}, {}, {}, {})",
        ts.a, ts.b, ts.c, ts.d, ts.e, ts.f
    ))
}
