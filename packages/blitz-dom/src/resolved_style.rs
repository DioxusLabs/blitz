//! Computation of *resolved* CSS property values, as exposed to JavaScript by
//! `getComputedStyle()`.
//!
//! For most properties the resolved value is the stylo computed value. For
//! layout-dependent properties (`width`/`height`, grid track sizes) it is the
//! *used* value, computed from the most recent layout.

use selectors::matching::QuirksMode;
use style::properties::{PropertyId, SourcePropertyDeclaration, parse_one_declaration_into};
use style::stylesheets::{CssRuleType, Origin};
use style::values::computed::length::CSSPixelLength;
use style_traits::{ParsingMode, ToCss};
use taffy::DetailedGridTracksInfo;

use blitz_traits::node_id::NodeId;

use crate::BaseDocument;

/// Serialize a used length (in CSS pixels) the way stylo serializes computed lengths
fn format_px(px: f32) -> String {
    CSSPixelLength::new(px).to_css_string()
}

/// Serialize the used sizes of a set of grid tracks (e.g. `100px 200px`)
fn format_grid_tracks<S: taffy::CheapCloneStr>(tracks: &DetailedGridTracksInfo<S>) -> String {
    if tracks.positions.is_empty() {
        return "none".to_string();
    }
    tracks
        .positions
        .iter()
        .map(|line| format_px(line.end - line.start))
        .collect::<Vec<_>>()
        .join(" ")
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
