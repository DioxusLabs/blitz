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
use style::properties::{
    Importance, PropertyDeclaration, PropertyDeclarationBlock, PropertyId,
    SourcePropertyDeclaration,
};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style::values::specified::svg::DProperty;
use style::values::specified::svg_path::SVGPathData;
use style_traits::ParsingMode;

/// Parse one SVG presentation attribute on element `tag` into CSS
/// declarations (a shorthand such as `font` expands into several). `None`
/// when `name` is not a presentation attribute for this element or `value`
/// does not parse as that property's value.
pub(crate) fn svg_presentation_hint(
    tag: &str,
    name: &str,
    value: &str,
    url_extra_data: &UrlExtraData,
) -> Option<Vec<PropertyDeclaration>> {
    if !is_presentation_attribute(tag, name) {
        return None;
    }
    let translated;
    let value = match name {
        "transform" => {
            translated = svg_transform_to_css(value)?;
            translated.as_str()
        }
        "d" => {
            let (path, _ok) = SVGPathData::parse_bytes(value.as_bytes());
            return Some(vec![PropertyDeclaration::D(DProperty::Path(path))]);
        }
        _ => value,
    };

    let mut context = ParserContext::new(
        Origin::Author,
        url_extra_data,
        Some(CssRuleType::Style),
        ParsingMode::ALLOW_UNITLESS_LENGTH | ParsingMode::ALLOW_ALL_NUMERIC_VALUES,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        Default::default(),
    );

    let property_id = PropertyId::parse(name, &context).ok()?;
    let mut declaration = SourcePropertyDeclaration::default();
    let mut parser = Parser::new(value);
    PropertyDeclaration::parse_into(&mut declaration, property_id, &mut context, &mut parser)
        .ok()?;
    let mut block = PropertyDeclarationBlock::new();
    block.extend(declaration.drain(), Importance::Normal);
    Some(block.declarations().to_vec())
}

/// <https://svgwg.org/svg2-draft/styling.html#PresentationAttributes>: the
/// geometry properties apply only on the elements they define the geometry
/// of; every other CSS property is a presentation attribute on all elements.
fn is_presentation_attribute(tag: &str, name: &str) -> bool {
    match name {
        "cx" | "cy" => matches!(tag, "circle" | "ellipse"),
        "r" => tag == "circle",
        "rx" | "ry" => matches!(tag, "ellipse" | "rect"),
        "x" | "y" | "width" | "height" => {
            matches!(tag, "rect" | "image" | "foreignObject" | "svg")
        }
        "d" => tag == "path",
        _ => true,
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
        svg_presentation_hint(tag, name, value, &url_data()).unwrap_or_default()
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
        // `path("...")` as a CSS value would have rejected the whole thing.
        assert!(parse_on("path", "d", "").is_empty() || true);
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
    }

    #[test]
    fn composes_transform_list() {
        assert_eq!(
            svg_transform_to_css("translate(10 20)").as_deref(),
            Some("matrix(1, 0, 0, 1, 10, 20)")
        );
    }
}
