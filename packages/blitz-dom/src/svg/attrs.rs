//! SVG presentation attributes → CSS declarations.
//!
//! Per SVG2 (<https://svgwg.org/svg2-draft/styling.html#PresentationAttributes>),
//! almost every SVG presentation attribute shares its name and value syntax
//! with the CSS property of the same name. So instead of hand-parsing each
//! one's value grammar (colors, lengths, keywords, ...), this resolves the
//! attribute name to a `PropertyId` and feeds the attribute value through
//! `PropertyDeclaration::parse_into`: the same parser
//! `ElementData::set_style_property` already uses for the `style="..."`
//! attribute (`node/element.rs`). Anything that doesn't parse is dropped,
//! not defaulted, construction never panics on malformed input.
//!
//! Geometry attributes (`x`, `y`, `width`, `height`, `cx`, `r`, `d`,
//! `points`, `viewBox`, `transform`, ...) are deliberately not here: most
//! have no CSS-property equivalent in this Stylo build, and `transform`'s
//! attribute syntax differs from the CSS property's anyway.
//! They're read directly off the DOM by the geometry pass instead

use selectors::matching::QuirksMode;
use style::parser::ParserContext;
use style::properties::{PropertyDeclaration, PropertyId, SourcePropertyDeclaration};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style_traits::ParsingMode;

/// SVG presentation attributes whose name and value syntax matches the CSS
/// property of the same name. Deliberately small for now just the paint
/// fundamentals.
const PRESENTATION_ATTRS: &[&str] = &[
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-opacity",
    "opacity",
    "visibility",
    "color",
];

/// Parse one SVG presentation attribute into a CSS declaration.
/// `None` if `name` isn't an attribute we map, or `value` doesn't
/// parse as the corresponding CSS property's value.
pub(crate) fn svg_presentation_hint(
    name: &str,
    value: &str,
    url_extra_data: &UrlExtraData,
) -> Option<SourcePropertyDeclaration> {
    if !PRESENTATION_ATTRS.contains(&name) {
        return None;
    }

    let context = ParserContext::new(
        Origin::Author,
        url_extra_data,
        Some(CssRuleType::Style),
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        Default::default(),
    );

    let property_id = PropertyId::parse(name, &context).ok()?;
    let mut declaration = SourcePropertyDeclaration::default();
    let mut input = cssparser::ParserInput::new(value);
    let mut parser = style::values::Parser::new(&mut input);
    PropertyDeclaration::parse_into(&mut declaration, property_id, &context, &mut parser).ok()?;
    Some(declaration)
}

#[cfg(test)]
mod tests {
    use super::svg_presentation_hint;
    use style::servo_arc::Arc as ServoArc;
    use style::stylesheets::UrlExtraData;

    fn test_url_data() -> UrlExtraData {
        UrlExtraData(ServoArc::new(url::Url::parse("about:blank").unwrap()))
    }

    #[test]
    fn maps_known_attr() {
        let url_data = test_url_data();
        assert!(svg_presentation_hint("fill", "red", &url_data).is_some());
    }

    #[test]
    fn rejects_unmapped_attr() {
        let url_data = test_url_data();
        // Geometry attrs are handled elsewhere, never as presentation hints.
        assert!(svg_presentation_hint("d", "M0 0", &url_data).is_none());
        assert!(svg_presentation_hint("viewBox", "0 0 1 1", &url_data).is_none());
    }

    #[test]
    fn rejects_invalid_value() {
        let url_data = test_url_data();
        assert!(svg_presentation_hint("fill", "not-a-color-or-keyword(", &url_data).is_none());
    }
}
