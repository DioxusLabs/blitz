//! Conversion functions from Stylo types to Parley types
use blitz_traits::node_id::NodeId;
use std::borrow::Cow;

use style::values::computed::Length;

use crate::node::TextBrush;

// Module of type aliases so we can refer to stylo types with nicer names
pub(crate) mod stylo {
    pub(crate) use style::computed_values::font_variant_caps::T as FontVariantCaps;
    pub(crate) use style::computed_values::font_variant_position::T as FontVariantPosition;
    pub(crate) use style::computed_values::text_wrap_mode::T as TextWrapMode;
    pub(crate) use style::computed_values::white_space_collapse::T as WhiteSpaceCollapse;
    pub(crate) use style::properties::ComputedValues;
    pub(crate) use style::properties::style_structs::Font;
    pub(crate) use style::values::computed::OverflowWrap;
    pub(crate) use style::values::computed::WordBreak;
    pub(crate) use style::values::computed::font::FontFeatureSettings;
    pub(crate) use style::values::computed::font::FontStretch;
    pub(crate) use style::values::computed::font::FontStyle;
    pub(crate) use style::values::computed::font::FontVariantEastAsian;
    pub(crate) use style::values::computed::font::FontVariantLigatures;
    pub(crate) use style::values::computed::font::FontVariantNumeric;
    pub(crate) use style::values::computed::font::FontVariationSettings;
    pub(crate) use style::values::computed::font::FontWeight;
    pub(crate) use style::values::computed::font::GenericFontFamily;
    pub(crate) use style::values::computed::font::LineHeight;
    pub(crate) use style::values::computed::font::SingleFontFamily;
}

pub(crate) mod parley {
    pub(crate) use parley::FontFeature;
    pub(crate) use parley::FontVariation;
    pub(crate) use parley::fontique::QueryFamily;
    pub(crate) use parley::setting::*;
    pub(crate) use parley::style::*;
}

pub(crate) fn generic_font_family(input: stylo::GenericFontFamily) -> parley::GenericFamily {
    match input {
        stylo::GenericFontFamily::None => parley::GenericFamily::SansSerif,
        stylo::GenericFontFamily::Serif => parley::GenericFamily::Serif,
        stylo::GenericFontFamily::SansSerif => parley::GenericFamily::SansSerif,
        stylo::GenericFontFamily::Monospace => parley::GenericFamily::Monospace,
        stylo::GenericFontFamily::Cursive => parley::GenericFamily::Cursive,
        stylo::GenericFontFamily::Fantasy => parley::GenericFamily::Fantasy,
        stylo::GenericFontFamily::SystemUi => parley::GenericFamily::SystemUi,
    }
}

pub(crate) fn query_font_family(input: &stylo::SingleFontFamily) -> parley::QueryFamily<'_> {
    match input {
        stylo::SingleFontFamily::FamilyName(name) => {
            'ret: {
                let name = name.name.as_ref();

                // Legacy web compatibility
                #[cfg(target_vendor = "apple")]
                if name == "-apple-system" {
                    break 'ret parley::QueryFamily::Generic(parley::GenericFamily::SystemUi);
                }
                #[cfg(target_os = "macos")]
                if name == "BlinkMacSystemFont" {
                    break 'ret parley::QueryFamily::Generic(parley::GenericFamily::SystemUi);
                }

                break 'ret parley::QueryFamily::Named(name);
            }
        }
        stylo::SingleFontFamily::Generic(generic) => {
            parley::QueryFamily::Generic(self::generic_font_family(*generic))
        }
    }
}

pub(crate) fn font_weight(input: stylo::FontWeight) -> parley::FontWeight {
    parley::FontWeight::new(input.value())
}

pub(crate) fn font_width(input: stylo::FontStretch) -> parley::FontWidth {
    parley::FontWidth::from_percentage(input.0.to_float())
}

pub(crate) fn font_style(input: stylo::FontStyle) -> parley::FontStyle {
    match input {
        stylo::FontStyle::NORMAL => parley::FontStyle::Normal,
        stylo::FontStyle::ITALIC => parley::FontStyle::Italic,
        val => parley::FontStyle::Oblique(Some(val.oblique_degrees())),
    }
}

pub(crate) fn font_variations(input: &stylo::FontVariationSettings) -> Vec<parley::FontVariation> {
    input
        .0
        .iter()
        .map(|v| parley::FontVariation {
            tag: parley::Tag::from_bytes(v.tag.0.to_be_bytes()),
            value: v.value,
        })
        .collect()
}

#[inline]
fn feature(tag: &[u8; 4], value: u16) -> parley::FontFeature {
    parley::FontFeature {
        tag: parley::Tag::from_bytes(*tag),
        value,
    }
}

/// Convert the `font-feature-settings` property (low-level OpenType feature control)
/// into a list of Parley font features.
pub(crate) fn font_feature_settings(
    input: &stylo::FontFeatureSettings,
    out: &mut Vec<parley::FontFeature>,
) {
    out.extend(input.0.iter().map(|v| parley::FontFeature {
        tag: parley::Tag::from_bytes(v.tag.0.to_be_bytes()),
        value: v.value as u16,
    }));
}

/// Map the `font-variant-ligatures` property to OpenType features.
pub(crate) fn font_variant_ligatures(
    input: stylo::FontVariantLigatures,
    out: &mut Vec<parley::FontFeature>,
) {
    use stylo::FontVariantLigatures as L;

    // `none` disables all optional ligature and contextual features.
    if input.contains(L::NONE) {
        out.push(feature(b"liga", 0));
        out.push(feature(b"clig", 0));
        out.push(feature(b"dlig", 0));
        out.push(feature(b"hlig", 0));
        out.push(feature(b"calt", 0));
        return;
    }

    if input.contains(L::COMMON_LIGATURES) {
        out.push(feature(b"liga", 1));
        out.push(feature(b"clig", 1));
    } else if input.contains(L::NO_COMMON_LIGATURES) {
        out.push(feature(b"liga", 0));
        out.push(feature(b"clig", 0));
    }
    if input.contains(L::DISCRETIONARY_LIGATURES) {
        out.push(feature(b"dlig", 1));
    } else if input.contains(L::NO_DISCRETIONARY_LIGATURES) {
        out.push(feature(b"dlig", 0));
    }
    if input.contains(L::HISTORICAL_LIGATURES) {
        out.push(feature(b"hlig", 1));
    } else if input.contains(L::NO_HISTORICAL_LIGATURES) {
        out.push(feature(b"hlig", 0));
    }
    if input.contains(L::CONTEXTUAL) {
        out.push(feature(b"calt", 1));
    } else if input.contains(L::NO_CONTEXTUAL) {
        out.push(feature(b"calt", 0));
    }
}

/// Map the `font-variant-caps` property to OpenType features.
pub(crate) fn font_variant_caps(input: stylo::FontVariantCaps, out: &mut Vec<parley::FontFeature>) {
    match input {
        stylo::FontVariantCaps::Normal => {}
        stylo::FontVariantCaps::SmallCaps => out.push(feature(b"smcp", 1)),
    }
}

/// Map the `font-variant-position` property to OpenType features.
pub(crate) fn font_variant_position(
    input: stylo::FontVariantPosition,
    out: &mut Vec<parley::FontFeature>,
) {
    match input {
        stylo::FontVariantPosition::Normal => {}
        stylo::FontVariantPosition::Sub => out.push(feature(b"subs", 1)),
        stylo::FontVariantPosition::Super => out.push(feature(b"sups", 1)),
    }
}

/// Map the `font-variant-numeric` property to OpenType features.
pub(crate) fn font_variant_numeric(
    input: stylo::FontVariantNumeric,
    out: &mut Vec<parley::FontFeature>,
) {
    use stylo::FontVariantNumeric as N;

    if input.contains(N::LINING_NUMS) {
        out.push(feature(b"lnum", 1));
    }
    if input.contains(N::OLDSTYLE_NUMS) {
        out.push(feature(b"onum", 1));
    }
    if input.contains(N::PROPORTIONAL_NUMS) {
        out.push(feature(b"pnum", 1));
    }
    if input.contains(N::TABULAR_NUMS) {
        out.push(feature(b"tnum", 1));
    }
    if input.contains(N::DIAGONAL_FRACTIONS) {
        out.push(feature(b"frac", 1));
    }
    if input.contains(N::STACKED_FRACTIONS) {
        out.push(feature(b"afrc", 1));
    }
    if input.contains(N::ORDINAL) {
        out.push(feature(b"ordn", 1));
    }
    if input.contains(N::SLASHED_ZERO) {
        out.push(feature(b"zero", 1));
    }
}

/// Map the `font-variant-east-asian` property to OpenType features.
pub(crate) fn font_variant_east_asian(
    input: stylo::FontVariantEastAsian,
    out: &mut Vec<parley::FontFeature>,
) {
    use stylo::FontVariantEastAsian as E;

    if input.contains(E::JIS78) {
        out.push(feature(b"jp78", 1));
    }
    if input.contains(E::JIS83) {
        out.push(feature(b"jp83", 1));
    }
    if input.contains(E::JIS90) {
        out.push(feature(b"jp90", 1));
    }
    if input.contains(E::JIS04) {
        out.push(feature(b"jp04", 1));
    }
    if input.contains(E::SIMPLIFIED) {
        out.push(feature(b"smpl", 1));
    }
    if input.contains(E::TRADITIONAL) {
        out.push(feature(b"trad", 1));
    }
    if input.contains(E::FULL_WIDTH) {
        out.push(feature(b"fwid", 1));
    }
    if input.contains(E::PROPORTIONAL_WIDTH) {
        out.push(feature(b"pwid", 1));
    }
    if input.contains(E::RUBY) {
        out.push(feature(b"ruby", 1));
    }
}

/// Build the full list of OpenType font features for a computed font style.
///
/// Per the CSS Fonts feature precedence rules
/// (<https://drafts.csswg.org/css-fonts/#feature-precedence>), features implied by the
/// high-level `font-variant-*` properties are applied first, and the low-level
/// `font-feature-settings` property is applied last so that it takes precedence.
///
/// Parley stably sorts the feature list by tag and HarfBuzz applies the last of any
/// duplicate tags, so appending `font-feature-settings` after the `font-variant-*`
/// features gives it the higher precedence required by the spec.
pub(crate) fn font_features(font_styles: &stylo::Font) -> Vec<parley::FontFeature> {
    let mut features = Vec::new();

    self::font_variant_ligatures(font_styles.font_variant_ligatures, &mut features);
    self::font_variant_caps(font_styles.font_variant_caps, &mut features);
    self::font_variant_numeric(font_styles.font_variant_numeric, &mut features);
    self::font_variant_east_asian(font_styles.font_variant_east_asian, &mut features);
    self::font_variant_position(font_styles.font_variant_position, &mut features);

    self::font_feature_settings(&font_styles.font_feature_settings, &mut features);

    features.sort_by_key(|feature| feature.tag);
    features.dedup_by_key(|feature| feature.tag);

    features
}

pub(crate) fn white_space_collapse(input: stylo::WhiteSpaceCollapse) -> parley::WhiteSpaceCollapse {
    match input {
        stylo::WhiteSpaceCollapse::Collapse => parley::WhiteSpaceCollapse::Collapse,
        stylo::WhiteSpaceCollapse::Preserve => parley::WhiteSpaceCollapse::Preserve,

        // TODO: Implement PreserveBreaks and BreakSpaces modes
        stylo::WhiteSpaceCollapse::PreserveBreaks => parley::WhiteSpaceCollapse::Preserve,
        stylo::WhiteSpaceCollapse::BreakSpaces => parley::WhiteSpaceCollapse::Preserve,
    }
}

pub(crate) fn style(
    span_id: NodeId,
    style: &stylo::ComputedValues,
) -> parley::TextStyle<'static, 'static, TextBrush> {
    let font_styles = style.get_font();
    let itext_styles = style.get_inherited_text();

    // Convert font size and line height
    let font_size = font_styles.font_size.used_size.0.px();
    let line_height = match font_styles.line_height {
        stylo::LineHeight::Normal => parley::LineHeight::FontSizeRelative(1.2),
        stylo::LineHeight::Number(num) => parley::LineHeight::FontSizeRelative(num.0),
        stylo::LineHeight::Length(value) => parley::LineHeight::Absolute(value.0.px()),
    };

    let letter_spacing = itext_styles
        .letter_spacing
        .0
        .resolve(Length::new(font_size))
        .px();

    let word_spacing = itext_styles
        .word_spacing
        .resolve(Length::new(font_size))
        .px();

    // Convert Bold/Italic
    let font_weight = self::font_weight(font_styles.font_weight);
    let font_style = self::font_style(font_styles.font_style);
    let font_width = self::font_width(font_styles.font_stretch);
    let font_variations = self::font_variations(&font_styles.font_variation_settings);
    let font_features = self::font_features(font_styles);

    // Convert font family
    let families: Vec<_> = font_styles
        .font_family
        .families
        .list
        .iter()
        .map(|family| match family {
            stylo::SingleFontFamily::FamilyName(name) => {
                'ret: {
                    let name = name.name.as_ref();

                    // Legacy web compatibility
                    #[cfg(target_vendor = "apple")]
                    if name == "-apple-system" {
                        break 'ret parley::FontFamilyName::Generic(
                            parley::GenericFamily::SystemUi,
                        );
                    }
                    #[cfg(target_os = "macos")]
                    if name == "BlinkMacSystemFont" {
                        break 'ret parley::FontFamilyName::Generic(
                            parley::GenericFamily::SystemUi,
                        );
                    }

                    break 'ret parley::FontFamilyName::Named(Cow::Owned(name.to_string()));
                }
            }
            stylo::SingleFontFamily::Generic(generic) => {
                parley::FontFamilyName::Generic(self::generic_font_family(*generic))
            }
        })
        .collect();

    // Wrapping and breaking
    let word_break = match itext_styles.word_break {
        stylo::WordBreak::Normal => parley::WordBreak::Normal,
        stylo::WordBreak::BreakAll => parley::WordBreak::BreakAll,
        stylo::WordBreak::KeepAll => parley::WordBreak::KeepAll,
    };
    let overflow_wrap = match itext_styles.overflow_wrap {
        stylo::OverflowWrap::Normal => parley::OverflowWrap::Normal,
        stylo::OverflowWrap::BreakWord => parley::OverflowWrap::BreakWord,
        stylo::OverflowWrap::Anywhere => parley::OverflowWrap::Anywhere,
    };
    let text_wrap_mode = match itext_styles.text_wrap_mode {
        stylo::TextWrapMode::Wrap => parley::TextWrapMode::Wrap,
        stylo::TextWrapMode::Nowrap => parley::TextWrapMode::NoWrap,
    };

    parley::TextStyle {
        // font_family: parley::FontFamily::Single(FontFamilyName::Generic(GenericFamily::SystemUi)),
        font_family: parley::FontFamily::List(Cow::Owned(families)),
        font_size,
        font_width,
        font_style,
        font_weight,
        font_variations: parley::FontVariations::List(Cow::Owned(font_variations)),
        font_features: parley::FontFeatures::List(Cow::Owned(font_features)),
        locale: Default::default(),
        line_height,
        word_spacing,
        letter_spacing,
        text_wrap_mode,
        overflow_wrap,
        word_break,

        // Contains NodeId
        brush: TextBrush::from_id(span_id),

        // We avoid sending these styles through Parley because they don't affect layout
        // and handling them separately allows us to update them without rebuilding the Parley layout.
        //
        // Instead of setting them here we pass the NodeId in the `brush` field and use that to read these
        // styles lazily when rendering.
        has_underline: Default::default(),
        underline_offset: Default::default(),
        underline_size: Default::default(),
        underline_brush: Default::default(),
        has_strikethrough: Default::default(),
        strikethrough_offset: Default::default(),
        strikethrough_size: Default::default(),
        strikethrough_brush: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use style::values::generics::font::{FeatureTagValue, FontSettings, FontTag};

    /// Collect the (tag, value) pairs from a feature list for easy assertions.
    fn pairs(features: &[parley::FontFeature]) -> Vec<([u8; 4], u16)> {
        features
            .iter()
            .map(|f| (f.tag.to_bytes(), f.value))
            .collect()
    }

    fn feature_settings(tags: &[(&[u8; 4], i32)]) -> stylo::FontFeatureSettings {
        FontSettings(
            tags.iter()
                .map(|(tag, value)| FeatureTagValue {
                    tag: FontTag(u32::from_be_bytes(**tag)),
                    value: *value,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
    }

    #[test]
    fn ligatures_none_disables_all() {
        let mut out = Vec::new();
        font_variant_ligatures(stylo::FontVariantLigatures::NONE, &mut out);
        assert_eq!(
            pairs(&out),
            vec![
                (*b"liga", 0),
                (*b"clig", 0),
                (*b"dlig", 0),
                (*b"hlig", 0),
                (*b"calt", 0),
            ]
        );
    }

    #[test]
    fn ligatures_normal_emits_nothing() {
        let mut out = Vec::new();
        font_variant_ligatures(stylo::FontVariantLigatures::NORMAL, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn ligatures_mixed_toggles() {
        let mut out = Vec::new();
        let input = stylo::FontVariantLigatures::COMMON_LIGATURES
            | stylo::FontVariantLigatures::NO_DISCRETIONARY_LIGATURES
            | stylo::FontVariantLigatures::HISTORICAL_LIGATURES;
        font_variant_ligatures(input, &mut out);
        assert_eq!(
            pairs(&out),
            vec![(*b"liga", 1), (*b"clig", 1), (*b"dlig", 0), (*b"hlig", 1)]
        );
    }

    #[test]
    fn caps_small_caps() {
        let mut out = Vec::new();
        font_variant_caps(stylo::FontVariantCaps::SmallCaps, &mut out);
        assert_eq!(pairs(&out), vec![(*b"smcp", 1)]);

        let mut out = Vec::new();
        font_variant_caps(stylo::FontVariantCaps::Normal, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn position_sub_and_super() {
        let mut out = Vec::new();
        font_variant_position(stylo::FontVariantPosition::Sub, &mut out);
        assert_eq!(pairs(&out), vec![(*b"subs", 1)]);

        let mut out = Vec::new();
        font_variant_position(stylo::FontVariantPosition::Super, &mut out);
        assert_eq!(pairs(&out), vec![(*b"sups", 1)]);
    }

    #[test]
    fn numeric_features() {
        let mut out = Vec::new();
        let input = stylo::FontVariantNumeric::OLDSTYLE_NUMS
            | stylo::FontVariantNumeric::TABULAR_NUMS
            | stylo::FontVariantNumeric::SLASHED_ZERO;
        font_variant_numeric(input, &mut out);
        assert_eq!(
            pairs(&out),
            vec![(*b"onum", 1), (*b"tnum", 1), (*b"zero", 1)]
        );
    }

    #[test]
    fn east_asian_features() {
        let mut out = Vec::new();
        let input =
            stylo::FontVariantEastAsian::JIS04 | stylo::FontVariantEastAsian::PROPORTIONAL_WIDTH;
        font_variant_east_asian(input, &mut out);
        assert_eq!(pairs(&out), vec![(*b"jp04", 1), (*b"pwid", 1)]);
    }

    #[test]
    fn feature_settings_map_directly() {
        let mut out = Vec::new();
        font_feature_settings(&feature_settings(&[(b"smcp", 1), (b"tnum", 0)]), &mut out);
        assert_eq!(pairs(&out), vec![(*b"smcp", 1), (*b"tnum", 0)]);
    }

    /// `font-feature-settings` has a higher precedence than the `font-variant-*`
    /// properties, so its entries must be appended *after* the variant-derived ones.
    /// Parley stably sorts by tag and HarfBuzz applies the last of any duplicate tags,
    /// so the trailing `font-feature-settings` value wins.
    #[test]
    fn feature_settings_override_variant_features() {
        let mut out = Vec::new();
        // font-variant-numeric: tabular-nums => tnum=1
        font_variant_numeric(stylo::FontVariantNumeric::TABULAR_NUMS, &mut out);
        // font-feature-settings: "tnum" 0 (should override the variant)
        font_feature_settings(&feature_settings(&[(b"tnum", 0)]), &mut out);

        let flat = pairs(&out);
        // Both are present, and the feature-settings value comes last.
        assert_eq!(flat, vec![(*b"tnum", 1), (*b"tnum", 0)]);
        let last_tnum = flat.iter().rev().find(|(tag, _)| tag == b"tnum").unwrap();
        assert_eq!(last_tnum.1, 0);
    }
}
