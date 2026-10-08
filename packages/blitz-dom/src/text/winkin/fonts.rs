//! winkin's fonts: the collection documents lay text out with, `@font-face` registration, and
//! the font metrics Stylo resolves font-relative units with.

use std::sync::{Arc, RwLock};

use app_units::Au;
use blitz_traits::net::Bytes;
use fontwich::{
    Attributes, Collection, FallbackKey, FallbackOverride, FallbackRequest, Family, FontBytes,
    FontStyle, FontWeight, FontWidth, LayerBuilder, Role,
};
use skrifa::MetadataProvider as _;
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::{GlyphMetrics, Metrics};
use style::device::servo::FontMetricsProvider;
use style::font_metrics::FontMetrics;
use style::properties::style_structs::Font as FontStyles;
use style::values::computed::font::{FontStyle as StyloFontStyle, QueryFontMetricsFlags};
use style::values::computed::{CSSPixelLength, Length};
use winkin::Context;
use winkin::style::FontFamilyName;

use super::TextContext;
use crate::net::FontFaceOverrides;
use crate::text::{DocumentText, TextFonts};

/// The fonts documents lay text out with: a fontwich collection. Clones share loaded fonts and
/// fallback answers, so hand a clone of one to every document rather than listing the platform's
/// fonts for each.
#[derive(Clone)]
pub struct FontContext {
    collection: Collection,
}

impl FontContext {
    /// The platform's fonts, where Blitz is built to read them (`system-fonts`), and no fonts
    /// otherwise.
    pub fn new() -> Self {
        #[cfg(all(feature = "system-fonts", not(target_arch = "wasm32")))]
        let collection = Collection::system();
        #[cfg(not(all(feature = "system-fonts", not(target_arch = "wasm32"))))]
        let collection = Collection::new();
        Self { collection }
    }

    /// The fonts in `collection`.
    pub fn from_collection(collection: Collection) -> Self {
        Self { collection }
    }

    /// The fontwich collection.
    pub fn collection(&self) -> &Collection {
        &self.collection
    }
}

impl Default for FontContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Fallback to the families of one layer, for every request: every generic family, and every
/// character, resolves to them.
struct OnlyFamilies(Vec<String>);

impl FallbackOverride for OnlyFamilies {
    fn families(&self, _key: &FallbackKey, collection: &Collection, out: &mut Vec<Family>) {
        out.extend(self.0.iter().filter_map(|name| collection.family(name)));
    }
}

impl TextFonts for FontContext {
    fn with_single_font(font_data: &[u8]) -> Self {
        let mut layer = LayerBuilder::new(Role::Application);
        let decoded = crate::decode_font_bytes(font_data).into_owned();
        let families = layer.add_data(FontBytes::new(decoded)).unwrap_or_default();
        let names = families
            .iter()
            .map(|family| family.name().to_string())
            .collect();
        layer.set_fallback_override(OnlyFamilies(names));
        Self::from_collection(Collection::new().with_layer(layer.snapshot()))
    }

    fn add_fonts(&mut self, font_data: impl Into<Bytes>) {
        let mut layer = LayerBuilder::new(Role::Application);
        let font_data = font_data.into();
        let bytes = match crate::decode_font_bytes(&font_data) {
            std::borrow::Cow::Owned(decoded) => FontBytes::new(decoded),
            std::borrow::Cow::Borrowed(_) => FontBytes::new(font_data),
        };
        if layer.add_data(bytes).is_ok() {
            self.collection.push(layer.snapshot());
        }
    }
}

impl DocumentText for TextContext {
    fn new(fonts: Option<FontContext>) -> Self {
        let given = fonts.unwrap_or_default();
        let mut shipped = LayerBuilder::new(Role::Application);
        let _ = shipped.add_data(FontBytes::from(crate::BULLET_FONT));
        let base = given.collection.clone().with_layer(shipped.snapshot());
        let document = LayerBuilder::new(Role::Document);
        let collection = base.clone().with_layer(document.snapshot());
        Self {
            cx: Context::new(collection.clone()),
            metrics: Arc::new(RwLock::new(collection)),
            given,
            base,
            document,
        }
    }

    fn fonts(&self) -> FontContext {
        self.given.clone()
    }

    fn add_web_font(&mut self, bytes: Bytes, overrides: &FontFaceOverrides) {
        // A rule with no family, or bytes that are not a font, adds nothing, as CSS falls back
        // past a face whose download is no font.
        let bytes = FontBytes::new(bytes);
        let added = match overrides.family_name.as_deref() {
            Some(family) => self
                .document
                .add_face(
                    family,
                    fontwich_descriptors(&overrides.descriptors),
                    Some((bytes, 0)),
                )
                .is_ok(),
            None => self.document.add_data(bytes).is_ok(),
        };
        if added {
            let collection = self.base.clone().with_layer(self.document.snapshot());
            self.cx.set_collection(collection.clone());
            *self.metrics.write().unwrap() = collection;
        }
    }

    fn font_metrics_provider(&self) -> Box<dyn FontMetricsProvider> {
        Box::new(WinkinFontMetricsProvider {
            fonts: self.metrics.clone(),
        })
    }
}

/// Font metrics from the fonts a document's text is laid out with: the first font in the family
/// list that maps the character measured, then the platform's standard font.
struct WinkinFontMetricsProvider {
    fonts: Arc<RwLock<Collection>>,
}

impl core::fmt::Debug for WinkinFontMetricsProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WinkinFontMetricsProvider")
    }
}

/// The bytes of a font, and its index in them.
type LoadedFont = (FontBytes, u32);

impl WinkinFontMetricsProvider {
    /// The first font of `families`, matched to `attributes`, that maps `ch`; or else the
    /// standard font's.
    fn font_for(
        fonts: &Collection,
        families: &[FontFamilyName<'_>],
        attributes: Attributes,
        ch: char,
    ) -> Option<LoadedFont> {
        let request = FallbackRequest::Standard(None);
        let standard = fonts.fallback(&fonts.key(&request)).first().cloned();
        families
            .iter()
            .filter_map(|name| fonts.resolve(name, &request))
            .chain(standard)
            .find_map(|family| {
                let font = family.match_font(attributes, true)?;
                if font.is_pending() || !font.serves(ch) || !font.charset().contains(ch) {
                    return None;
                }
                Some((font.load()?, font.index()))
            })
    }
}

impl FontMetricsProvider for WinkinFontMetricsProvider {
    fn query_font_metrics(
        &self,
        _vertical: bool,
        font_styles: &FontStyles,
        font_size: CSSPixelLength,
        _flags: QueryFontMetricsFlags,
    ) -> FontMetrics {
        let fonts = self.fonts.read().unwrap();
        let families: Vec<FontFamilyName<'_>> = font_styles
            .font_family
            .families
            .list
            .iter()
            .map(super::style::font_family_name)
            .collect();
        let attributes = Attributes {
            width: FontWidth::from_percentage(font_styles.font_width.0.to_float()),
            weight: FontWeight::new(font_styles.font_weight.value()),
            style: match font_styles.font_style {
                StyloFontStyle::NORMAL => FontStyle::Normal,
                StyloFontStyle::ITALIC => FontStyle::Italic,
                oblique => FontStyle::Oblique(Some(oblique.oblique_degrees())),
            },
        };
        let variations: Vec<(skrifa::Tag, f32)> = font_styles
            .font_variation_settings
            .0
            .iter()
            .map(|setting| {
                (
                    skrifa::Tag::from_be_bytes(setting.tag.0.to_be_bytes()),
                    setting.value,
                )
            })
            .collect();

        // The advance of `ch`, scaled as winkin scales a shaped advance: font units times size
        // over units per em.
        let advance_of = |ch: char| -> Option<f32> {
            let (bytes, index) = Self::font_for(&fonts, &families, attributes, ch)?;
            let font = skrifa::FontRef::from_index(bytes.data(), index).ok()?;
            let location = font.axes().location(variations.iter().copied());
            let location = LocationRef::from(&location);
            let upem = Metrics::new(&font, Size::unscaled(), location).units_per_em;
            if upem == 0 {
                return None;
            }
            let glyph = font.charmap().map(ch)?;
            let advance =
                GlyphMetrics::new(&font, Size::unscaled(), location).advance_width(glyph)?;
            Some(advance * (font_size.px() / f32::from(upem)))
        };
        let metrics_of = |ch: char| -> Option<(f32, Option<f32>, Option<f32>)> {
            let (bytes, index) = Self::font_for(&fonts, &families, attributes, ch)?;
            let font = skrifa::FontRef::from_index(bytes.data(), index).ok()?;
            let location = font.axes().location(variations.iter().copied());
            let metrics = Metrics::new(
                &font,
                Size::new(font_size.px()),
                LocationRef::from(&location),
            );
            Some((metrics.ascent, metrics.x_height, metrics.cap_height))
        };

        let zero_advance = advance_of('0');
        let ic_advance = advance_of('\u{6C34}');
        let (ascent, x_height, cap_height) = metrics_of(' ').unwrap_or((0.0, None, None));

        FontMetrics {
            ascent: CSSPixelLength::new(ascent),
            x_height: x_height.filter(|xh| *xh != 0.0).map(CSSPixelLength::new),
            cap_height: cap_height.map(CSSPixelLength::new),
            zero_advance_measure: zero_advance.map(CSSPixelLength::new),
            ic_width: ic_advance.map(CSSPixelLength::new),
            script_percent_scale_down: None,
            script_script_percent_scale_down: None,
        }
    }

    fn base_size_for_generic(
        &self,
        generic: style::values::computed::font::GenericFontFamily,
    ) -> Length {
        let size = match generic {
            style::values::computed::font::GenericFontFamily::Monospace => 13.0,
            _ => 16.0,
        };
        Length::from(Au::from_f32_px(size))
    }
}

/// An `@font-face` rule's descriptors as fontwich declares a face with them.
fn fontwich_descriptors(descriptors: &crate::text::FaceDescriptors) -> fontwich::FaceDescriptors {
    use crate::text::parlance::FontStyle;
    use fontwich::{FaceStyle, FontWeight, FontWidth};
    use winkin::style::{FontFeature, FontVariation, Tag};

    /// The angle CSS gives an `oblique` that names none.
    const OBLIQUE: f32 = 14.0;
    fontwich::FaceDescriptors {
        weight: descriptors
            .weight
            .map(|(min, max)| (FontWeight::new(min.value()), FontWeight::new(max.value()))),
        width: descriptors.width.map(|(min, max)| {
            (
                FontWidth::from_ratio(min.ratio()),
                FontWidth::from_ratio(max.ratio()),
            )
        }),
        style: descriptors.style.map(|style| match style {
            (FontStyle::Normal, _) => FaceStyle::Normal,
            (FontStyle::Italic, _) => FaceStyle::Italic,
            (FontStyle::Oblique(min), max) => {
                let max = match max {
                    FontStyle::Oblique(max) => max.or(min),
                    _ => min,
                };
                FaceStyle::Oblique(min.unwrap_or(OBLIQUE), max.unwrap_or(OBLIQUE))
            }
        }),
        unicode_range: descriptors.unicode_range.clone(),
        feature_settings: descriptors
            .feature_settings
            .iter()
            .map(|feature| FontFeature::new(Tag::from_bytes(feature.tag.to_bytes()), feature.value))
            .collect(),
        variation_settings: descriptors
            .variation_settings
            .iter()
            .map(|variation| FontVariation {
                tag: Tag::from_bytes(variation.tag.to_bytes()),
                value: variation.value,
            })
            .collect(),
        size_adjust: descriptors.size_adjust,
        ascent_override: descriptors.ascent_override,
        descent_override: descriptors.descent_override,
        line_gap_override: descriptors.line_gap_override,
    }
}
