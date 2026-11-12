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
        // `scale` on <feDisplacementMap>) stays a plain attribute.
        "alignment-baseline"
        | "baseline-shift"
        | "clip-path"
        | "clip-rule"
        | "color"
        | "color-interpolation"
        | "color-interpolation-filters"
        | "cursor"
        | "direction"
        | "display"
        | "dominant-baseline"
        | "fill"
        | "fill-opacity"
        | "fill-rule"
        | "filter"
        | "flood-color"
        | "flood-opacity"
        | "font-family"
        | "font-size"
        | "font-size-adjust"
        | "font-stretch"
        | "font-style"
        | "font-variant"
        | "font-weight"
        | "glyph-orientation-horizontal"
        | "glyph-orientation-vertical"
        | "image-rendering"
        | "letter-spacing"
        | "lighting-color"
        | "marker-end"
        | "marker-mid"
        | "marker-start"
        | "mask"
        | "mask-type"
        | "opacity"
        | "overflow"
        | "paint-order"
        | "pointer-events"
        | "shape-rendering"
        | "stop-color"
        | "stop-opacity"
        | "stroke"
        | "stroke-dasharray"
        | "stroke-dashoffset"
        | "stroke-linecap"
        | "stroke-linejoin"
        | "stroke-miterlimit"
        | "stroke-opacity"
        | "stroke-width"
        | "text-anchor"
        | "text-decoration"
        | "text-overflow"
        | "text-rendering"
        | "transform"
        | "transform-origin"
        | "unicode-bidi"
        | "vector-effect"
        | "visibility"
        | "white-space"
        | "word-spacing"
        | "writing-mode" => true,
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

#[cfg(test)]
mod tests {
    use super::*;
    use style::servo_arc::Arc as ServoArc;

    fn url_data() -> UrlExtraData {
        UrlExtraData(ServoArc::new(url::Url::parse("about:blank").unwrap()))
    }

    fn parse(name: &str, value: &str) -> Vec<PropertyDeclaration> {
        let tag = match name {
            "cx" | "cy" | "r" => "circle",
            "rx" | "ry" | "x" | "y" | "width" | "height" => "rect",
            "d" => "path",
            _ => "g",
        };
        parse_on(tag, name, value)
    }

    fn parse_on(tag: &str, name: &str, value: &str) -> Vec<PropertyDeclaration> {
        let mut out = SourcePropertyDeclaration::default();
        svg_presentation_hint(tag, name, value, &url_data(), &mut out);
        out.drain().declarations.collect()
    }

    #[test]
    fn only_svg_presentation_attributes_map_to_css() {
        assert!(parse_on("rect", "all", "unset").is_empty());
        assert!(parse_on("svg", "padding", "30").is_empty());
        assert!(parse_on("svg", "background", "red").is_empty());
        assert!(parse_on("svg", "margin-left", "200").is_empty());
        assert!(parse_on("svg", "border", "5px solid blue").is_empty());
        assert!(parse_on("rect", "position", "absolute").is_empty());
        assert!(parse_on("text", "rotate", "45").is_empty());
        assert!(parse_on("g", "font", "12px serif").is_empty());
        assert!(!parse_on("g", "stroke-linecap", "round").is_empty());
        assert!(!parse_on("g", "transform-origin", "10px 10px").is_empty());
    }

    #[test]
    fn geometry_attrs_only_on_their_elements() {
        assert!(!parse_on("circle", "cx", "5").is_empty());
        assert!(!parse_on("ellipse", "cx", "5").is_empty());
        assert!(parse_on("text", "x", "5").is_empty());
        assert!(parse_on("use", "x", "5").is_empty());
        assert!(parse_on("mask", "width", "5").is_empty());
        assert!(!parse_on("foreignObject", "width", "5").is_empty());
        assert!(!parse_on("svg", "height", "5").is_empty());
        assert!(parse_on("ellipse", "r", "5").is_empty());
        assert!(parse_on("circle", "rx", "5").is_empty());
        assert!(!parse_on("rect", "rx", "5").is_empty());
        assert!(parse_on("rect", "d", "M 0 0").is_empty());
        // Non-geometry properties apply everywhere.
        assert!(!parse_on("text", "fill", "red").is_empty());
    }

    #[test]
    fn path_data_keeps_commands_before_first_error() {
        let decls = parse("d", "M 0 0 L 10 10 Q bogus");
        let [PropertyDeclaration::D(DProperty::Path(path))] = &decls[..] else {
            panic!("{decls:?}");
        };
        assert_eq!(path.commands().len(), 2);
    }

    #[test]
    fn maps_paint_and_geometry_attrs() {
        assert!(matches!(
            parse("fill", "red")[..],
            [PropertyDeclaration::Fill(_)]
        ));
        assert!(matches!(
            parse("stroke-width", "2")[..],
            [PropertyDeclaration::StrokeWidth(_)]
        ));
        assert!(matches!(
            parse("cx", "50%")[..],
            [PropertyDeclaration::Cx(_)]
        ));
        assert!(matches!(
            parse("d", "M 0 0 L 10 10")[..],
            [PropertyDeclaration::D(_)]
        ));
        assert!(matches!(
            parse("transform", "translate(10 20) scale(2)")[..],
            [PropertyDeclaration::Transform(_)]
        ));
    }

    #[test]
    fn rejects_non_css_and_invalid_attrs() {
        assert!(parse("viewBox", "0 0 1 1").is_empty());
        assert!(parse("points", "0,0 1,1").is_empty());
        assert!(parse("fill", "not-a-color(").is_empty());
        assert!(parse("transform", "bogus(1)").is_empty());
        // A negative radius is an error, so the attribute is ignored (`auto`),
        // not clamped: `ALLOW_ALL_NUMERIC_VALUES` is for SMIL, not for this.
        assert!(parse("rx", "-30").is_empty());
        assert!(parse("r", "-1").is_empty());
    }

    #[test]
    fn composes_transform_list() {
        assert_eq!(
            svg_transform_to_css("translate(10 20)").as_deref(),
            Some("matrix(1, 0, 0, 1, 10, 20)")
        );
    }
}
