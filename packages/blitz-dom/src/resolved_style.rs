//! Computation of *resolved* CSS property values, as exposed to JavaScript by
//! `getComputedStyle()`.
//!
//! For most properties the resolved value is the stylo computed value. For
//! layout-dependent properties (`width`/`height`, grid track sizes) it is the
//! *used* value, computed from the most recent layout.

use cssparser::{Parser, ParserInput};
use selectors::matching::QuirksMode;
use style::parser::ParserContext;
use style::properties::declaration_block::{Importance, parse_style_attribute};
use style::properties::{
    PropertyDeclarationBlock, PropertyId, SourcePropertyDeclaration, parse_one_declaration_into,
};
use style::stylesheets::supports_rule::parse_condition_or_declaration;
use style::stylesheets::{CssRuleType, Origin};
use style::values::computed::length::CSSPixelLength;
use style_traits::{CssStringWriter, ParsingMode, ToCss};
use taffy::DetailedGridTracksInfo;

use blitz_traits::node_id::NodeId;

use crate::BaseDocument;

/// Serialize a used length (in CSS pixels) the way stylo serializes computed lengths
fn format_px(px: f32) -> String {
    CSSPixelLength::new(px).to_css_string()
}

/// Serialize the used sizes of a set of grid tracks (e.g. `100px 200px`)
fn format_grid_tracks(tracks: &DetailedGridTracksInfo) -> String {
    if tracks.sizes.is_empty() {
        return "none".to_string();
    }
    tracks
        .sizes
        .iter()
        .map(|size| format_px(*size))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Check whether `name` names a CSS property supported by the style engine.
/// Custom properties (`--*`) are not considered "supported" here, matching the
/// behaviour of the `in` operator on `CSSStyleDeclaration` objects in browsers.
pub fn css_property_is_supported(name: &str) -> bool {
    matches!(
        PropertyId::parse_enabled_for_all_content(name),
        Ok(property_id) if !matches!(property_id, PropertyId::Custom(_))
    )
}

impl BaseDocument {
    /// Check whether `value` is a valid value for the CSS property `property`.
    /// Used by CSSOM APIs (`element.style.setProperty` and friends), which must
    /// ignore invalid declarations.
    pub fn css_declaration_is_valid(&self, property: &str, value: &str) -> bool {
        let Ok(property_id) = PropertyId::parse_enabled_for_all_content(property) else {
            return false;
        };
        let mut declarations = SourcePropertyDeclaration::default();
        parse_one_declaration_into(
            &mut declarations,
            property_id,
            value,
            Origin::Author,
            &self.url.url_extra_data(),
            None,
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        )
        .is_ok()
    }

    /// Parse a `style` attribute string into a [`PropertyDeclarationBlock`].
    /// Invalid declarations are dropped (per CSS error recovery).
    fn parse_style_attr_block(&self, style_attr: &str) -> PropertyDeclarationBlock {
        parse_style_attribute(
            style_attr,
            &self.url.url_extra_data(),
            None,
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        )
    }

    /// The canonical CSSOM serialization of a `style` attribute string
    /// (`CSSStyleDeclaration.cssText` getter).
    pub fn style_attr_serialize(&self, style_attr: &str) -> String {
        let block = self.parse_style_attr_block(style_attr);
        let mut css = CssStringWriter::new();
        let _ = block.to_css(&mut css);
        css
    }

    /// The canonical serialization of `property`'s value in a `style` attribute
    /// string (`CSSStyleDeclaration.getPropertyValue()`). Handles shorthands.
    /// Returns an empty string if the property is not set or not recognised.
    pub fn style_attr_get_property(&self, style_attr: &str, property: &str) -> String {
        let Ok(property_id) = PropertyId::parse_enabled_for_all_content(property) else {
            return String::new();
        };
        let block = self.parse_style_attr_block(style_attr);
        let mut css = CssStringWriter::new();
        let _ = block.property_value_to_css(&property_id, &mut css);
        css
    }

    /// Set `property` to `value` in a `style` attribute string
    /// (`CSSStyleDeclaration.setProperty()`), expanding shorthands.
    ///
    /// Returns the new (canonically serialized) style attribute, or `None` if
    /// the declaration was invalid (in which case the style is unchanged, per
    /// CSSOM). An empty `value` removes the property.
    pub fn style_attr_set_property(
        &self,
        style_attr: &str,
        property: &str,
        value: &str,
        important: bool,
    ) -> Option<String> {
        let property_id = PropertyId::parse_enabled_for_all_content(property).ok()?;
        let mut block = self.parse_style_attr_block(style_attr);

        if value.trim().is_empty() {
            if let Some(first_declaration) = block.first_declaration_to_remove(&property_id) {
                block.remove_property(&property_id, first_declaration);
            }
        } else {
            let mut source = SourcePropertyDeclaration::default();
            parse_one_declaration_into(
                &mut source,
                property_id,
                value,
                Origin::Author,
                &self.url.url_extra_data(),
                None,
                ParsingMode::DEFAULT,
                QuirksMode::NoQuirks,
                CssRuleType::Style,
            )
            .ok()?;
            let importance = if important {
                Importance::Important
            } else {
                Importance::Normal
            };
            block.extend(source.drain(), importance);
        }

        let mut css = CssStringWriter::new();
        let _ = block.to_css(&mut css);
        Some(css)
    }

    /// Remove `property` from a `style` attribute string
    /// (`CSSStyleDeclaration.removeProperty()`), removing all longhands if it
    /// is a shorthand.
    ///
    /// Returns the new (canonically serialized) style attribute and the removed
    /// property's previous serialized value.
    pub fn style_attr_remove_property(
        &self,
        style_attr: &str,
        property: &str,
    ) -> Option<(String, String)> {
        let property_id = PropertyId::parse_enabled_for_all_content(property).ok()?;
        let mut block = self.parse_style_attr_block(style_attr);

        let mut removed_value = CssStringWriter::new();
        let _ = block.property_value_to_css(&property_id, &mut removed_value);
        if let Some(first_declaration) = block.first_declaration_to_remove(&property_id) {
            block.remove_property(&property_id, first_declaration);
        }

        let mut css = CssStringWriter::new();
        let _ = block.to_css(&mut css);
        Some((css, removed_value))
    }

    /// Evaluate a `@supports` condition (e.g. `(display: grid)`,
    /// `selector(:hover)`) or a bare declaration (e.g. `display: grid`), as
    /// used by the CSSOM `CSS.supports(conditionText)` API. Returns `false`
    /// for unparseable conditions.
    pub fn css_supports_condition(&self, condition: &str) -> bool {
        let mut input = ParserInput::new(condition);
        let mut parser = Parser::new(&mut input);
        let Ok(condition) = parser.parse_entirely(parse_condition_or_declaration) else {
            return false;
        };

        let url_data = self.url.url_extra_data();
        let context = ParserContext::new(
            Origin::Author,
            &url_data,
            Some(CssRuleType::Style),
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            Default::default(),
            None,
            None,
            Default::default(),
        );
        condition.eval(&context)
    }

    /// Compute the resolved value of a CSS property for the given node, as exposed
    /// by `getComputedStyle()`. Returns an empty string for unknown properties and
    /// for nodes without styles.
    ///
    /// Layout-dependent properties (`width`/`height`, `grid-template-rows`/`columns`)
    /// resolve to *used* values, so [`resolve`](Self::resolve) should be called
    /// before this method to ensure layout is up to date.
    pub fn resolved_style_value(&self, node_id: NodeId, property_name: &str) -> String {
        let Some(node) = self.get_node(node_id) else {
            return String::new();
        };
        let Some(styles) = node.primary_styles() else {
            return String::new();
        };

        let has_layout_box =
            node.flags.is_in_document() && node.style().display != taffy::Display::None;

        // Layout-dependent "used value" special cases
        match property_name {
            "grid-template-columns" | "grid-template-rows"
                if node.style().display == taffy::Display::Grid =>
            {
                if let Some(info) = node
                    .element_data()
                    .and_then(|data| data.detailed_grid_info.as_ref())
                {
                    let tracks = if property_name == "grid-template-columns" {
                        &info.columns
                    } else {
                        &info.rows
                    };
                    return format_grid_tracks(tracks);
                }
            }
            "width" | "height" if has_layout_box => {
                // Used value: content-box size
                let layout = node.final_layout();
                let size = if property_name == "width" {
                    layout.size.width
                        - layout.border.left
                        - layout.border.right
                        - layout.padding.left
                        - layout.padding.right
                } else {
                    layout.size.height
                        - layout.border.top
                        - layout.border.bottom
                        - layout.padding.top
                        - layout.padding.bottom
                };
                return format_px(size.max(0.0));
            }
            _ => {}
        }

        // General case: serialize the stylo computed value
        let Ok(property_id) = PropertyId::parse_enabled_for_all_content(property_name) else {
            return String::new();
        };
        match property_id.as_shorthand() {
            // Resolved values are not currently computed for shorthand properties
            Ok(_shorthand) => String::new(),
            Err(declaration_id) => styles.computed_value_to_string(declaration_id),
        }
    }
}
