//! Document-wide text contexts and fonts.

use blitz_traits::net::Bytes;
use std::ops::RangeInclusive;

use parlance::{FontFeature, FontStyle, FontVariation, FontWeight, FontWidth, Tag};

use super::backend;

/// The fonts documents lay out text with. Clones share their fonts.
///
/// This is the text backend's own type, Parley's `FontContext` under Parley, and its methods
/// beyond [`TextFonts`] are the backend's: code written for every backend uses [`TextFonts`].
pub type FontContext = backend::FontContext;

/// What a document keeps for its text: its fonts, and the backend's contexts.
pub(crate) type TextContext = backend::TextContext;

/// The fonts documents lay out text with.
pub trait TextFonts: Clone + Default {
    /// Fonts with only the fonts in `font_data`, decoded from WOFF or WOFF2 where it is, which
    /// every generic family resolves to, and no platform fonts: the setup for WASM, where browsers
    /// do not expose them.
    fn with_single_font(font_data: &[u8]) -> Self;

    /// Adds the fonts in `font_data`, decoded from WOFF or WOFF2 where it is, under the family
    /// names they declare. Fonts that need no decoding are kept without a copy.
    fn add_fonts(&mut self, font_data: impl Into<Bytes>);
}

/// What a document keeps for its text: its fonts, and the contexts the backend builds and
/// breaks text in.
pub(crate) trait DocumentText: Sized {
    /// The context for a document handed `fonts`, or the default fonts.
    fn new(fonts: Option<FontContext>) -> Self;

    /// The fonts the document was handed, as it hands them to its iframes.
    fn fonts(&self) -> FontContext;

    /// Adds a web font that has loaded, under its `@font-face` rule's descriptors.
    fn add_web_font(&mut self, bytes: Bytes, overrides: &crate::net::FontFaceOverrides);

    /// What Stylo measures font-relative units (`ex`, `ch`, `cap`, `ic`) with.
    fn font_metrics_provider(&self) -> Box<dyn style::device::servo::FontMetricsProvider>;
}

/// The descriptors of an `@font-face` rule that a text backend registers a face by, beyond its
/// family name, in the font value types the backends share.
///
/// A descriptor left out, or one whose `calc()` cannot be resolved without an element, is `None`
/// or empty: the font's own value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceDescriptors {
    /// The `font-weight` descriptor, as its lowest and highest weight; a single weight is both.
    pub weight: Option<(FontWeight, FontWeight)>,
    /// The `font-width` (`font-stretch`) descriptor, as its narrowest and widest width; a single
    /// width is both.
    pub width: Option<(FontWidth, FontWidth)>,
    /// The `font-style` descriptor, as its lowest and highest style; only an oblique's angle can
    /// differ between them.
    pub style: Option<(FontStyle, FontStyle)>,
    /// The code point ranges of the `unicode-range` descriptor; empty where it allows every one.
    pub unicode_range: Vec<RangeInclusive<u32>>,
    /// The `font-feature-settings` descriptor.
    pub feature_settings: Vec<FontFeature>,
    /// The `font-variation-settings` descriptor.
    pub variation_settings: Vec<FontVariation>,
    /// The `size-adjust` descriptor, as a ratio: `1.0` is `100%`.
    pub size_adjust: Option<f32>,
    /// The `ascent-override` descriptor, as a fraction of the em.
    pub ascent_override: Option<f32>,
    /// The `descent-override` descriptor, as a fraction of the em.
    pub descent_override: Option<f32>,
    /// The `line-gap-override` descriptor, as a fraction of the em.
    pub line_gap_override: Option<f32>,
}

impl FaceDescriptors {
    /// Reads the descriptors of an `@font-face` rule.
    pub(crate) fn from_rule(descriptors: &style::font_face::Descriptors) -> Self {
        use style::font_face::FontStyleRange;
        use style::values::specified::font::MetricsOverride;
        let metric = |value: &Option<MetricsOverride>| match value {
            Some(MetricsOverride::Override(percentage)) => percentage.0.get(),
            _ => None,
        };
        Self {
            weight: descriptors
                .font_weight
                .as_ref()
                .and_then(|range| range.compute())
                .map(|range| {
                    (
                        FontWeight::new(range.0.value()),
                        FontWeight::new(range.1.value()),
                    )
                }),
            style: descriptors.font_style.as_ref().map(|style| match style {
                FontStyleRange::Italic => (FontStyle::Italic, FontStyle::Italic),
                FontStyleRange::Oblique(min, max) => {
                    let (min, max) = (min.degrees(), max.degrees());
                    // Stylo parses `normal` as an oblique of no angle.
                    if min.is_none_or(|angle| angle == 0.0) && max.is_none_or(|angle| angle == 0.0)
                    {
                        (FontStyle::Normal, FontStyle::Normal)
                    } else {
                        (FontStyle::Oblique(min), FontStyle::Oblique(max.or(min)))
                    }
                }
            }),
            width: descriptors
                .font_width
                .as_ref()
                .and_then(|range| range.compute())
                .map(|range| {
                    (
                        FontWidth::from_percentage(range.0.to_percentage().0 * 100.0),
                        FontWidth::from_percentage(range.1.to_percentage().0 * 100.0),
                    )
                }),
            unicode_range: descriptors
                .unicode_range
                .as_ref()
                .map(|ranges| ranges.iter().map(|range| range.start..=range.end).collect())
                .unwrap_or_default(),
            // The rule's settings are specified values, whose numbers are known once parsed.
            feature_settings: descriptors
                .font_feature_settings
                .as_ref()
                .map(|settings| {
                    settings
                        .0
                        .iter()
                        .filter_map(|setting| {
                            let value = setting.value.resolve()?;
                            Some(FontFeature::new(
                                Tag::from_bytes(setting.tag.0.to_be_bytes()),
                                value.clamp(0, i32::from(u16::MAX)) as u16,
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            variation_settings: descriptors
                .font_variation_settings
                .as_ref()
                .map(|settings| {
                    settings
                        .0
                        .iter()
                        .filter_map(|setting| {
                            Some(FontVariation::new(
                                Tag::from_bytes(setting.tag.0.to_be_bytes()),
                                setting.value.get()?,
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            size_adjust: descriptors
                .size_adjust
                .as_ref()
                .and_then(|percentage| percentage.0.get()),
            ascent_override: metric(&descriptors.ascent_override),
            descent_override: metric(&descriptors.descent_override),
            line_gap_override: metric(&descriptors.line_gap_override),
        }
    }
}

/// Fonts with only the fonts in `font_data`, which every generic family resolves to, and no
/// platform fonts: the standard setup for WASM, where browsers do not expose them. WOFF and WOFF2
/// are decoded.
pub fn build_single_font_ctx(font_data: &[u8]) -> FontContext {
    FontContext::with_single_font(font_data)
}

#[cfg(test)]
mod tests {
    use style::stylesheets::{CssRule, Origin, StylesheetInDocument as _};

    use parlance::{FontStyle, FontWeight};

    use super::FaceDescriptors;
    use crate::{BaseDocument, DocumentConfig};

    /// The descriptors of an `@font-face` rule with `declarations`.
    fn face(declarations: &str) -> FaceDescriptors {
        let doc = BaseDocument::new(DocumentConfig::default());
        let css = format!("@font-face {{ font-family: face; {declarations} }}");
        let sheet = doc.make_stylesheet(css, Origin::Author);
        let guard = doc.guard.read();
        let rules = sheet.0.contents(&guard).rules(&guard);
        rules
            .iter()
            .find_map(|rule| match rule {
                CssRule::FontFace(face) => Some(FaceDescriptors::from_rule(
                    &face.read_with(&guard).descriptors,
                )),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn face_weight_is_a_value_or_a_range() {
        assert_eq!(face("").weight, None);
        let weight = |min, max| Some((FontWeight::new(min), FontWeight::new(max)));
        assert_eq!(face("font-weight: 700").weight, weight(700.0, 700.0));
        assert_eq!(face("font-weight: bold").weight, weight(700.0, 700.0));
        assert_eq!(face("font-weight: 300 600").weight, weight(300.0, 600.0));
    }

    #[test]
    fn face_style_is_normal_italic_or_oblique() {
        let style = |style| Some((style, style));
        assert_eq!(face("").style, None);
        assert_eq!(face("font-style: normal").style, style(FontStyle::Normal));
        assert_eq!(face("font-style: italic").style, style(FontStyle::Italic));
        assert_eq!(
            face("font-style: oblique").style,
            style(FontStyle::Oblique(Some(14.0)))
        );
        assert_eq!(
            face("font-style: oblique 10deg 20deg").style,
            Some((
                FontStyle::Oblique(Some(10.0)),
                FontStyle::Oblique(Some(20.0))
            ))
        );
    }
}
