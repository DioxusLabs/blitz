//! Computation of *resolved* CSS property values, as exposed to JavaScript by
//! `getComputedStyle()`.
//!
//! For most properties the resolved value is the stylo computed value. For
//! layout-dependent properties (`width`/`height`, grid track sizes) it is the
//! *used* value, computed from the most recent layout.

use cssparser::{Parser, ParserInput};
use selectors::matching::QuirksMode;
use style::computed_values::box_sizing::T as BoxSizing;
use style::computed_values::position::T as Position;
use style::parser::ParserContext;
use style::properties::declaration_block::{Importance, parse_style_attribute};
use style::properties::{
    ComputedValues, PropertyDeclaration, PropertyDeclarationBlock, PropertyId, ShorthandId,
    SourcePropertyDeclaration, parse_one_declaration_into,
};
use style::stylesheets::supports_rule::parse_condition_or_declaration;
use style::stylesheets::{CssRuleType, Origin};
use style::values::computed::LengthPercentage;
use style::values::computed::length::CSSPixelLength;
use style::values::generics::position::Inset as GenericInset;
use style::values::resolved;
use style::values::specified::box_::DisplayInside;
use style_traits::{CssStringWriter, ParsingMode, ToCss};

use blitz_traits::node_id::NodeId;

use crate::BaseDocument;

/// Serialize a used length (in CSS pixels) the way stylo serializes computed lengths
fn format_px(px: f32) -> String {
    CSSPixelLength::new(px).to_css_string()
}

/// Resolve a computed inset value to CSS pixels against the given percentage
/// basis. Returns `None` for `auto` (and unsupported anchor functions).
fn resolve_inset<P>(val: &GenericInset<P, LengthPercentage>, basis: f32) -> Option<f32> {
    match val {
        GenericInset::LengthPercentage(lp) => Some(lp.resolve(CSSPixelLength::new(basis)).px()),
        _ => None,
    }
}

/// Serialize a transform matrix component rounded to 6 decimal places (as
/// browsers do when serializing resolved transform matrices)
fn format_matrix_component(v: f64) -> String {
    let rounded = (v * 1e6).round() / 1e6;
    // Avoid "-0"
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    format!("{rounded}")
}

/// Serialize a used transform matrix the way `getComputedStyle()` does
/// (`matrix(...)` for 2D transforms, `matrix3d(...)` otherwise).
fn format_matrix(m: &euclid::default::Transform3D<f64>, is_3d: bool) -> String {
    let c = format_matrix_component;
    if is_3d {
        format!(
            "matrix3d({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
            c(m.m11),
            c(m.m12),
            c(m.m13),
            c(m.m14),
            c(m.m21),
            c(m.m22),
            c(m.m23),
            c(m.m24),
            c(m.m31),
            c(m.m32),
            c(m.m33),
            c(m.m34),
            c(m.m41),
            c(m.m42),
            c(m.m43),
            c(m.m44)
        )
    } else {
        format!(
            "matrix({}, {}, {}, {}, {}, {})",
            c(m.m11),
            c(m.m12),
            c(m.m21),
            c(m.m22),
            c(m.m41),
            c(m.m42)
        )
    }
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

        let display = styles.clone_display();
        let has_layout_box = node.flags.is_in_document() && !display.is_none();

        // Layout-dependent "used value" special cases
        match property_name {
            "grid-template-columns" | "grid-template-rows"
                if display.inside() == DisplayInside::Grid =>
            {
                if let Some(info) =
                    node.element_data()
                        .and_then(|data| match &data.detailed_layout_info {
                            taffy::DetailedLayoutInfo::Grid(info) => Some(info),
                            _ => None,
                        })
                {
                    return if property_name == "grid-template-columns" {
                        info.grid_template_columns()
                    } else {
                        info.grid_template_rows()
                    };
                }
            }
            "width" | "height" if has_layout_box => {
                // Used value: the layout size interpreted according to `box-sizing`
                // (border-box size for `border-box`, content-box size for `content-box`)
                let layout = node.final_layout();
                let border_box = styles.get_position().box_sizing == BoxSizing::BorderBox;
                let size = if property_name == "width" {
                    if border_box {
                        layout.size.width
                    } else {
                        layout.size.width
                            - layout.border.left
                            - layout.border.right
                            - layout.padding.left
                            - layout.padding.right
                    }
                } else if border_box {
                    layout.size.height
                } else {
                    layout.size.height
                        - layout.border.top
                        - layout.border.bottom
                        - layout.padding.top
                        - layout.padding.bottom
                };
                return format_px(size.max(0.0));
            }
            "margin-top" | "margin-right" | "margin-bottom" | "margin-left" if has_layout_box => {
                // Used value: the margin resolved by layout (percentages and
                // `auto` margins resolved to lengths)
                let layout = node.final_layout();
                let margin = match property_name {
                    "margin-top" => layout.margin.top,
                    "margin-right" => layout.margin.right,
                    "margin-bottom" => layout.margin.bottom,
                    "margin-left" => layout.margin.left,
                    _ => unreachable!(),
                };
                return format_px(margin);
            }
            "top" | "right" | "bottom" | "left" if has_layout_box => {
                let position = styles.clone_position();
                let parent_layout = node
                    .layout_parent
                    .get()
                    .and_then(|id| self.get_node(id))
                    .map(|parent| *parent.final_layout());

                match position {
                    // Used value: the relative offset. The non-`auto` side of
                    // each axis wins (`top`/`left` take precedence when both
                    // are set) and the opposite side resolves to its negation.
                    Position::Relative => {
                        let (cb_width, cb_height) = parent_layout
                            .map(|pl| {
                                (
                                    pl.size.width
                                        - pl.border.left
                                        - pl.border.right
                                        - pl.padding.left
                                        - pl.padding.right,
                                    pl.size.height
                                        - pl.border.top
                                        - pl.border.bottom
                                        - pl.padding.top
                                        - pl.padding.bottom,
                                )
                            })
                            .unwrap_or((0.0, 0.0));
                        let pos_styles = styles.get_position();
                        let is_vertical = matches!(property_name, "top" | "bottom");
                        let basis = if is_vertical { cb_height } else { cb_width };
                        let (start, end) = if is_vertical {
                            (
                                resolve_inset(&pos_styles.top, basis),
                                resolve_inset(&pos_styles.bottom, basis),
                            )
                        } else {
                            (
                                resolve_inset(&pos_styles.left, basis),
                                resolve_inset(&pos_styles.right, basis),
                            )
                        };
                        let used_start = match (start, end) {
                            (Some(start), _) => start,
                            (None, Some(end)) => -end,
                            (None, None) => 0.0,
                        };
                        let used = if matches!(property_name, "top" | "left") {
                            used_start
                        } else {
                            -used_start
                        };
                        return format_px(used);
                    }
                    // A specified (non-`auto`) inset resolves as-is (with
                    // percentages resolved against the containing block), even
                    // when overconstrained. An `auto` inset resolves to the
                    // used distance between the box's margin edge and the
                    // corresponding edge of the containing block's padding box.
                    Position::Absolute | Position::Fixed => {
                        if let Some(pl) = parent_layout {
                            let cb_width = pl.size.width - pl.border.left - pl.border.right;
                            let cb_height = pl.size.height - pl.border.top - pl.border.bottom;

                            let pos_styles = styles.get_position();
                            let (inset, basis) = match property_name {
                                "top" => (&pos_styles.top, cb_height),
                                "bottom" => (&pos_styles.bottom, cb_height),
                                "left" => (&pos_styles.left, cb_width),
                                "right" => (&pos_styles.right, cb_width),
                                _ => unreachable!(),
                            };
                            if let Some(value) = resolve_inset(inset, basis) {
                                return format_px(value);
                            }

                            let layout = node.final_layout();
                            let margin_box_top =
                                layout.location.y - layout.margin.top - pl.border.top;
                            let margin_box_left =
                                layout.location.x - layout.margin.left - pl.border.left;
                            let margin_box_width =
                                layout.margin.left + layout.size.width + layout.margin.right;
                            let margin_box_height =
                                layout.margin.top + layout.size.height + layout.margin.bottom;
                            let used = match property_name {
                                "top" => margin_box_top,
                                "bottom" => cb_height - margin_box_top - margin_box_height,
                                "left" => margin_box_left,
                                "right" => cb_width - margin_box_left - margin_box_width,
                                _ => unreachable!(),
                            };
                            return format_px(used);
                        }
                    }
                    // `static` and `sticky` boxes resolve to the computed value
                    _ => {}
                }
            }
            "transform" if has_layout_box => {
                let transform = &styles.get_box().transform;
                if !transform.0.is_empty() {
                    // Used value: the transform list resolved to a matrix, with
                    // percentages resolved against the border box
                    let layout = node.final_layout();
                    let reference_box = euclid::Rect::new(
                        euclid::Point2D::new(CSSPixelLength::new(0.0), CSSPixelLength::new(0.0)),
                        euclid::Size2D::new(
                            CSSPixelLength::new(layout.size.width),
                            CSSPixelLength::new(layout.size.height),
                        ),
                    );
                    if let Ok((matrix, is_3d)) =
                        transform.to_transform_3d_matrix_f64(Some(&reference_box))
                    {
                        return format_matrix(&matrix, is_3d);
                    }
                }
            }
            _ => {}
        }

        // General case: serialize the stylo computed value
        let Ok(property_id) = PropertyId::parse_enabled_for_all_content(property_name) else {
            return String::new();
        };
        match property_id.as_shorthand() {
            // Serialize shorthands from the resolved values of their longhands
            Ok(shorthand) => serialize_resolved_shorthand(&styles, shorthand),
            Err(declaration_id) => styles.computed_value_to_string(declaration_id),
        }
    }
}

/// Serialize the resolved value of a shorthand property by resolving each of
/// its longhands and re-serializing them as the shorthand (as
/// `getComputedStyle()` does for shorthands). Returns an empty string if the
/// longhand values cannot be represented as the shorthand.
fn serialize_resolved_shorthand(styles: &ComputedValues, shorthand: ShorthandId) -> String {
    // `all` cannot be serialized from longhands
    if shorthand == ShorthandId::All {
        return String::new();
    }

    let declarations: Vec<PropertyDeclaration> = shorthand
        .longhands()
        .map(|longhand| {
            let mut context = resolved::Context {
                style: styles,
                for_property: PropertyId::NonCustom(longhand.into()),
                current_longhand: Some(longhand),
            };
            styles.computed_or_resolved_declaration(longhand, Some(&mut context))
        })
        .collect();
    let declaration_refs: Vec<&PropertyDeclaration> = declarations.iter().collect();

    let mut css = CssStringWriter::new();
    let _ = shorthand.longhands_to_css(&declaration_refs, &mut css);
    css
}
