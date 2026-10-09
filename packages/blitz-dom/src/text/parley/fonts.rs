//! Parley's fonts: the collection documents lay text out with, `@font-face` registration, and
//! the font metrics Stylo resolves font-relative units with.

#[cfg(feature = "parallel-construct")]
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use app_units::Au;
use blitz_traits::net::Bytes;
use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use parley::{FontContext, FontVariation, LayoutContext};
use skrifa::MetadataProvider as _;
use skrifa::charmap::Charmap;
use style::properties::style_structs::Font as FontStyles;
use style::{
    device::servo::FontMetricsProvider,
    font_metrics::FontMetrics,
    values::computed::{CSSPixelLength, font::QueryFontMetricsFlags},
};

/// Query fontique for the fonts matching `font_styles` (family list and attributes).
fn query_for_font_styles<'a>(
    font_ctx: &'a mut FontContext,
    font_styles: &'a FontStyles,
) -> parley::fontique::Query<'a> {
    use parley::fontique::Attributes;

    let mut query = font_ctx.collection.query(&mut font_ctx.source_cache);
    let families = font_styles
        .font_family
        .families
        .iter()
        .map(stylo_to_parley::query_font_family);
    query.set_families(families);
    query.set_attributes(Attributes {
        width: stylo_to_parley::font_width(font_styles.font_width),
        weight: stylo_to_parley::font_weight(font_styles.font_weight),
        style: stylo_to_parley::font_style(font_styles.font_style),
    });
    query
}

use super::{TextContext, style as stylo_to_parley};
use crate::net::FontFaceOverrides;
use crate::text::{DocumentText, TextFonts};

/// An `@font-face` rule's descriptors that Parley registers a face by, beyond its family name.
#[derive(Clone, Debug, Default)]
pub struct FaceDescriptors {
    /// `font-weight` descriptor as a single CSS weight (100–900). Stylo
    /// parses this as a range; we record the lower bound, which equals the
    /// upper bound in the common single-value case.
    pub weight: Option<f32>,
    /// `font-style` descriptor mapped to fontique's `FontStyle`.
    pub style: Option<parley::fontique::FontStyle>,
}

/// The descriptors of an `@font-face` rule that Parley registers a face by.
pub(crate) fn face_descriptors(descriptor: &style::font_face::Descriptors) -> FaceDescriptors {
    FaceDescriptors {
        weight: descriptor
            .font_weight
            .as_ref()
            .and_then(|range| range.0.compute().map(|w| w.value())),
        style: descriptor.font_style.as_ref().map(stylo_to_fontique_style),
    }
}

/// Translate stylo's `@font-face` `font-style` descriptor into the fontique
/// `FontStyle` enum used by parley. Stylo encodes Italic and Oblique-with-
/// angle distinctly; CSS's bare `normal` is parsed as `Oblique(0deg, 0deg)`
/// by stylo (see the `FontStyle::parse` impl in stylo's `font_face.rs`), so
/// that pattern is treated as `Normal` here.
fn stylo_to_fontique_style(
    style: &style::font_face::FontStyleRange,
) -> parley::fontique::FontStyle {
    use parley::fontique::FontStyle as Fq;
    use style::font_face::FontStyleRange;
    match style {
        FontStyleRange::Italic => Fq::Italic,
        FontStyleRange::Oblique(min, max) => {
            let angle = min.degrees();
            // Stylo emits `Oblique(0deg, 0deg)` for the literal CSS `normal`
            // keyword. Map that back to `Normal` so parley's font matching
            // doesn't misclassify upright fonts.
            if angle.is_none_or(|a| a == 0.0) && max.degrees().is_none_or(|a| a == 0.0) {
                Fq::Normal
            } else {
                Fq::Oblique(angle)
            }
        }
    }
}

impl TextFonts for FontContext {
    fn with_single_font(font_data: &[u8]) -> Self {
        let mut ctx = FontContext {
            source_cache: SourceCache::new_shared(),
            collection: Collection::new(CollectionOptions {
                shared: false,
                system_fonts: false,
            }),
        };
        let decoded = crate::decode_font_bytes(font_data).into_owned();
        let registered = ctx
            .collection
            .register_fonts(Blob::new(Arc::new(decoded) as _), None);
        let family_ids: Vec<_> = registered.iter().map(|(id, _)| *id).collect();
        for generic in [
            GenericFamily::SansSerif,
            GenericFamily::Serif,
            GenericFamily::Monospace,
            GenericFamily::SystemUi,
        ] {
            ctx.collection
                .append_generic_families(generic, family_ids.iter().copied());
        }
        ctx
    }

    fn add_fonts(&mut self, font_data: &[u8]) {
        let decoded = crate::decode_font_bytes(font_data).into_owned();
        self.collection
            .register_fonts(Blob::new(Arc::new(decoded) as _), None);
    }
}

impl DocumentText for TextContext {
    fn new(fonts: Option<FontContext>) -> Self {
        let font_ctx = fonts
            .map(|mut font_ctx| {
                font_ctx.source_cache.make_shared();
                // font_ctx.collection.make_shared();
                font_ctx
            })
            .unwrap_or_else(|| {
                let mut font_ctx = FontContext {
                    source_cache: SourceCache::new_shared(),
                    collection: Collection::new(CollectionOptions {
                        shared: false,
                        system_fonts: cfg!(all(
                            feature = "system-fonts",
                            not(target_arch = "wasm32")
                        )),
                    }),
                };
                font_ctx
                    .collection
                    .register_fonts(Blob::new(Arc::new(crate::BULLET_FONT) as _), None);
                font_ctx
            });
        Self {
            font_ctx: Arc::new(Mutex::new(font_ctx)),
            #[cfg(feature = "parallel-construct")]
            thread_font_contexts: thread_local::ThreadLocal::new(),
            layout_ctx: LayoutContext::new(),
        }
    }

    fn fonts(&self) -> FontContext {
        self.font_ctx.lock().unwrap().clone()
    }

    fn add_web_font(&mut self, bytes: Bytes, overrides: &FontFaceOverrides) {
        let font = Blob::new(Arc::new(bytes));

        // Build a `FontInfoOverride` from the `@font-face` descriptors
        // captured during stylesheet parsing. Without this, parley
        // reads the family name from the TTF's own metadata, which
        // means CSS `font-family: 'Avenir Book'` won't match a font
        // file that internally identifies as `Avenir 45 Book`.
        let weight_override = overrides
            .descriptors
            .weight
            .map(parley::fontique::FontWeight::new);
        let info_override = parley::fontique::FontInfoOverride {
            family_name: overrides.family_name.as_deref(),
            weight: weight_override,
            style: overrides.descriptors.style,
            ..Default::default()
        };

        // TODO: Investigate eliminating double-box
        let mut global_font_ctx = self.font_ctx.lock().unwrap();
        global_font_ctx
            .collection
            .register_fonts(font.clone(), Some(info_override));

        #[cfg(feature = "parallel-construct")]
        {
            rayon::broadcast(|_ctx| {
                let mut font_ctx = self
                    .thread_font_contexts
                    .get_or(|| RefCell::new(Box::new(global_font_ctx.clone())))
                    .borrow_mut();
                font_ctx
                    .collection
                    .register_fonts(font.clone(), Some(info_override));
            });
        }
        drop(global_font_ctx);
    }

    fn font_metrics_provider(&self) -> Box<dyn FontMetricsProvider> {
        Box::new(BlitzFontMetricsProvider {
            font_ctx: self.font_ctx.clone(),
        })
    }
}

#[derive(Clone)]
pub(crate) struct BlitzFontMetricsProvider {
    pub(crate) font_ctx: Arc<Mutex<FontContext>>,
}

impl core::fmt::Debug for BlitzFontMetricsProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlitzFontMetricsProvider")
    }
}

impl FontMetricsProvider for BlitzFontMetricsProvider {
    fn query_font_metrics(
        &self,
        _vertical: bool,
        font_styles: &FontStyles,
        font_size: CSSPixelLength,
        _flags: QueryFontMetricsFlags,
    ) -> FontMetrics {
        use parley::fontique::{Query, QueryFont, QueryStatus};
        use skrifa::instance::{LocationRef, Size};
        use skrifa::metrics::{GlyphMetrics, Metrics};

        // Lock font_ctx. Explicit reborrow required for borrow checker.
        let mut font_ctx = self.font_ctx.lock().unwrap();
        let font_ctx = &mut *font_ctx;

        // Query fontique for the font that matches the font styles
        let mut query = query_for_font_styles(font_ctx, font_styles);
        // let fb_script = crate::swash_convert::script_to_fontique(script);
        // let fb_language = locale.and_then(crate::swash_convert::locale_to_fontique);
        // query.set_fallbacks(fontique::FallbackKey::new(fb_script, fb_language.as_ref()));

        let variations = stylo_to_parley::font_variations(&font_styles.font_variation_settings);
        // let features = self.rcx.features(style.font_features).unwrap_or(&[]);

        // fn name_of(font_ref: &skrifa::FontRef) -> String {
        //     use skrifa::string::StringId;
        //     font_ref
        //         .localized_strings(StringId::POSTSCRIPT_NAME)
        //         .english_or_first()
        //         .unwrap()
        //         .chars()
        //         .collect()
        // }

        fn find_font_for(query: &mut Query, ch: char) -> Option<QueryFont> {
            let mut font = None;
            query.matches_with(|q_font: &QueryFont| {
                use skrifa::MetadataProvider;

                let Ok(font_ref) = skrifa::FontRef::from_index(q_font.blob.as_ref(), q_font.index)
                else {
                    return QueryStatus::Continue;
                };

                let charmap = font_ref.charmap();
                if charmap.map(ch).is_some() {
                    font = Some(q_font.clone());
                    QueryStatus::Stop
                } else {
                    QueryStatus::Continue
                }
            });
            font
        }

        /// Scales the advance the same way as Parley's shaped glyph advances (font units × size / upem)
        /// rather than with skrifa's FreeType-compatible fixed-point scale, so that `ch`/`ic`
        /// lengths exactly match the width of the corresponding laid-out text.
        fn advance_of(
            query: &mut Query,
            ch: char,
            font_size: f32,
            variations: &[FontVariation],
        ) -> Option<f32> {
            let font = find_font_for(query, ch)?;
            let font_ref = skrifa::FontRef::from_index(font.blob.as_ref(), font.index).ok()?;
            let location = font_ref.axes().location(
                variations
                    .iter()
                    .map(|v| (skrifa::Tag::from_be_bytes(v.tag.to_bytes()), v.value)),
            );
            let location_ref = LocationRef::from(&location);
            let upem = Metrics::new(&font_ref, Size::unscaled(), location_ref).units_per_em;
            if upem == 0 {
                return None;
            }
            let glyph_metrics = GlyphMetrics::new(&font_ref, Size::unscaled(), location_ref);
            let char_map = Charmap::new(&font_ref);
            let glyph_id = char_map.map(ch)?;
            let advance = glyph_metrics.advance_width(glyph_id)?;
            Some(advance * (font_size / upem as f32))
        }

        fn metrics_of(
            query: &mut Query,
            ch: char,
            font_size: Size,
            variations: &[FontVariation],
        ) -> Option<(f32, Option<f32>, Option<f32>)> {
            let font = find_font_for(query, ch)?;
            let font_ref = skrifa::FontRef::from_index(font.blob.as_ref(), font.index).ok()?;
            let location = font_ref.axes().location(
                variations
                    .iter()
                    .map(|v| (skrifa::Tag::from_be_bytes(v.tag.to_bytes()), v.value)),
            );
            let location_ref = LocationRef::from(&location);
            let metrics = Metrics::new(&font_ref, font_size, location_ref);
            Some((metrics.ascent, metrics.x_height, metrics.cap_height))
        }

        let zero_advance = advance_of(&mut query, '0', font_size.px(), &variations);
        let ic_advance = advance_of(&mut query, '\u{6C34}', font_size.px(), &variations);
        let (ascent, x_height, cap_height) =
            metrics_of(&mut query, ' ', Size::new(font_size.px()), &variations)
                .unwrap_or((0.0, None, None));

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
    ) -> style::values::computed::Length {
        let size = match generic {
            style::values::computed::font::GenericFontFamily::Monospace => 13.0,
            _ => 16.0,
        };
        style::values::computed::Length::from(Au::from_f32_px(size))
    }
}

#[cfg(test)]
mod font_face_override_tests {
    use crate::net::{FontFaceOverrides, Resource, ResourceLoadResponse};
    use crate::{BaseDocument, DocumentConfig};

    use super::FaceDescriptors;

    /// Regression-pin for the `@font-face` descriptor-honouring fix.
    ///
    /// The bug was that `Resource::Font` carried only the raw font bytes,
    /// so `load_resource` registered fonts with `info_override = None` and
    /// parley fell back to the TTF's internal `name` table. After the fix,
    /// `Resource::Font` carries `FontFaceOverrides` and `load_resource`
    /// builds a `FontInfoOverride` from them — meaning a CSS-declared
    /// `font-family` alias wins over the file's own metadata.
    ///
    /// We drive `load_resource` directly with a fabricated response rather
    /// than go through HTML parsing → `fetch_font_face`, because the
    /// downstream HTML parser lives in `blitz-html` (would be a circular
    /// crate dependency). The mapping from `@font-face` descriptors into
    /// `FontFaceOverrides` is covered by the unit tests in this module; this
    /// test pins the load-side of the pipeline.
    #[test]
    fn font_face_overrides_alias_family_name() {
        const ALIAS: &str = "AliasedFamily";

        let mut document = BaseDocument::new(DocumentConfig::default());

        // Sanity: the alias name is not registered before we feed the font.
        {
            let mut ctx = document.text.font_ctx.lock().unwrap();
            assert!(
                ctx.collection.family_id(ALIAS).is_none(),
                "alias must not exist before registration",
            );
        }

        // Drive `load_resource` with a `Resource::Font` whose overrides
        // assert the CSS-side family name. We use the bullet font as a
        // valid font payload — its internal `name` table is irrelevant to
        // the assertion; what matters is whether the override wins.
        let response = ResourceLoadResponse {
            request_id: 0,
            node_id: None,
            resolved_url: Some(String::from("test://aliased-family")),
            result: Ok(Resource::Font(
                blitz_traits::net::Bytes::from_static(crate::BULLET_FONT),
                FontFaceOverrides {
                    family_name: Some(String::from(ALIAS)),
                    descriptors: FaceDescriptors {
                        weight: Some(800.0),
                        style: Some(parley::fontique::FontStyle::Italic),
                    },
                },
            )),
        };
        document.load_resource(response);

        // The override must have taken effect: parley's `Collection` now
        // resolves the CSS-declared alias to a registered family.
        let mut ctx = document.text.font_ctx.lock().unwrap();
        let family_id = ctx
            .collection
            .family_id(ALIAS)
            .expect("CSS-declared family name should be registered as a family alias");
        let resolved_name = ctx
            .collection
            .family_name(family_id)
            .expect("family id should resolve back to a name");
        assert_eq!(
            resolved_name, ALIAS,
            "registered family should report the CSS-declared name, \
             not the font file's internal `name` table entry",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::stylo_to_fontique_style;
    use parley::fontique::FontStyle as Fq;
    use style::font_face::FontStyleRange;
    use style::values::specified::Angle;

    fn oblique(min_deg: f32, max_deg: f32) -> FontStyleRange {
        FontStyleRange::Oblique(Angle::from_degrees(min_deg), Angle::from_degrees(max_deg))
    }

    #[test]
    fn italic_maps_to_italic() {
        assert_eq!(stylo_to_fontique_style(&FontStyleRange::Italic), Fq::Italic,);
    }

    #[test]
    fn oblique_zero_zero_maps_to_normal() {
        // Stylo parses bare CSS `normal` as `Oblique(0deg, 0deg)`; the
        // helper must round-trip that back to `FontStyle::Normal` so
        // parley's matching doesn't misclassify upright fonts.
        assert_eq!(stylo_to_fontique_style(&oblique(0.0, 0.0)), Fq::Normal);
    }

    #[test]
    fn oblique_single_angle_maps_to_oblique_with_min() {
        assert_eq!(
            stylo_to_fontique_style(&oblique(14.0, 14.0)),
            Fq::Oblique(Some(14.0)),
        );
    }

    #[test]
    fn oblique_range_uses_min_angle() {
        // For a range, fontique's single-angle representation takes the
        // lower bound — confirm `min` (not `max`) is what gets through.
        assert_eq!(
            stylo_to_fontique_style(&oblique(10.0, 20.0)),
            Fq::Oblique(Some(10.0)),
        );
    }
}
