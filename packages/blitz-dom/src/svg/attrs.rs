//! SVG presentation attributes.
//!
//! - Attributes backed by a servo-enabled longhand (the vast majority: `fill`,
//!   `stroke`, `cx`, `d`, `opacity`, ...) go through the *generic* route:
//!   parse the attribute name as a `PropertyId` and hand the value to Stylo's
//!   own declaration parser (the same parser `style="..."` attribute values
//!   go through, see `node/element.rs::set_style_property`), so we get full
//!   CSS-value-syntax support (calc(), `currentColor`, units, ...) for free
//!   and never hand-roll a parser that silently diverges from the cascade's.
//! - The 17 "group C" properties/shorthand (`stop-color`, `marker-start`,
//!   `paint-order`, `text-anchor`, ...) are `engine = "gecko"` in this Stylo
//!   build (servo has no parseable value type for them at all), so
//!   `PropertyId::parse` always fails for them. Those are read as raw
//!   strings directly off the DOM attribute at geometry/paint time instead
//!   (see `geometry.rs`, `resolve.rs`, `render/svg.rs` in blitz-paint).

use cssparser::ParserInput;
use markup5ever::LocalName;
use selectors::matching::QuirksMode;
use style::parser::ParserContext;
use style::properties::{PropertyDeclaration, PropertyId, SourcePropertyDeclaration};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style_traits::ParsingMode;

/// SVG2 presentation-attribute names that are *not* CSS properties at all
/// (geometry helpers, unit/reference attrs, `id`/`class`/`style` which are
/// handled elsewhere). Used purely to skip parser dispatch quickly; not a
/// correctness requirement since `PropertyId::parse` already rejects
/// anything that isn't a real CSS property.
fn is_plausible_presentation_attr(local: &LocalName) -> bool {
    !matches!(
        local.as_ref(),
        "viewBox"
            | "preserveAspectRatio"
            | "points"
            | "x1"
            | "y1"
            | "x2"
            | "y2"
            | "dx"
            | "dy"
            | "rotate"
            | "textLength"
            | "lengthAdjust"
            | "gradientUnits"
            | "gradientTransform"
            | "spreadMethod"
            | "patternUnits"
            | "patternContentUnits"
            | "patternTransform"
            | "clipPathUnits"
            | "maskUnits"
            | "maskContentUnits"
            | "markerUnits"
            | "markerWidth"
            | "markerHeight"
            | "refX"
            | "refY"
            | "orient"
            | "filterUnits"
            | "primitiveUnits"
            | "in"
            | "in2"
            | "result"
            | "href"
            | "xlink:href"
            | "startOffset"
            | "method"
            | "spacing"
            | "side"
            | "offset"
            | "id"
            | "class"
            | "style"
    )
}

/// Attempt to parse `local="value"` as an SVG presentation attribute.
/// Returns the parsed declarations (a shorthand can expand to several
/// longhands) on success. Returns `None` if `local` is not a parseable
/// CSS property on this build (group-C attrs) or `value` doesn't parse
/// for that property, callers should simply skip the attribute in
/// that case, not treat it as an error.
///
/// Attribute-name lookup is case-sensitive on the `LocalName` as-is:
/// html5ever's foreign-content adjustment already restores SVG's camelCase
/// spellings (`viewBox`, not `viewbox`), so no case-folding happens here.
pub fn svg_presentation_hint(
    local: &LocalName,
    value: &str,
    url_extra_data: &UrlExtraData,
) -> Option<SourcePropertyDeclaration> {
    if !is_plausible_presentation_attr(local) {
        return None;
    }

    // SVG presentation attributes accept unitless numbers (user units, which
    // map 1:1 to CSS px) and out-of-range numeric values are clamped rather
    // than rejected, unlike normal CSS length/number parsing.
    let context = ParserContext::new(
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

    let property_id = PropertyId::parse(local.as_ref(), &context).ok()?;

    let mut source_property_declaration = SourcePropertyDeclaration::default();
    let mut input = ParserInput::new(value);
    let mut parser = style::values::Parser::new(&mut input);
    PropertyDeclaration::parse_into(
        &mut source_property_declaration,
        property_id,
        &context,
        &mut parser,
    )
    .ok()?;

    Some(source_property_declaration)
}

/// Read a "group C" attribute directly off the element, bypassing
/// the cascade entirely (these are not parseable CSS properties on this
/// Stylo build, so they never reach the generic route above). This means
/// they do **not** cascade, inherit, or animate.
pub fn raw_attr<'a>(attrs: &'a [crate::node::Attribute], local: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|a| a.name.local.as_ref() == local)
        .map(|a| a.value.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_and_reference_attrs_are_not_presentation_attrs() {
        for name in [
            "viewBox",
            "gradientUnits",
            "href",
            "xlink:href",
            "patternTransform",
            "markerWidth",
        ] {
            assert!(
                !is_plausible_presentation_attr(&LocalName::from(name)),
                "{name} should not be treated as a presentation attribute"
            );
        }
    }

    #[test]
    fn styling_attrs_are_plausible_presentation_attrs() {
        for name in [
            "fill",
            "stroke",
            "cx",
            "cy",
            "r",
            "opacity",
            "d",
            "transform",
        ] {
            assert!(is_plausible_presentation_attr(&LocalName::from(name)));
        }
    }
}
