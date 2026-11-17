use blitz_traits::node_id::NodeId;
use selectors::context::QuirksMode;
use std::sync::atomic::Ordering as Ao;
use std::{
    io::Cursor,
    sync::{Arc, atomic::AtomicUsize, mpsc::Sender},
};
use style::{
    font_face::{
        FontFaceSourceFormat, FontFaceSourceFormatKeyword, FontStyleRange, Source, SourceList,
    },
    media_queries::MediaList,
    servo_arc::Arc as ServoArc,
    shared_lock::SharedRwLock,
    shared_lock::{Locked, SharedRwLockReadGuard},
    stylesheets::{
        CssRule, DocumentStyleSheet, ImportRule, Origin, Stylesheet, StylesheetInDocument,
        StylesheetLoader as ServoStylesheetLoader, UrlExtraData,
        import_rule::{ImportLayer, ImportSheet, ImportSupportsCondition},
    },
    values::{CssUrl, SourceLocation},
};

use blitz_traits::net::{AbortSignal, Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::ShellProvider;

use url::Url;

use crate::{document::DocumentEvent, util::ImageType};

pub(crate) fn stamped_request(url: Url, signal: Option<&AbortSignal>) -> Request {
    let mut req = Request::get(url);
    if let Some(sig) = signal {
        req = req.signal(sig.clone());
    }
    req
}

/// Carries `@font-face` descriptors from CSS parsing through to font
/// registration so `parley::Collection::register_fonts` can alias the bytes
/// under the `font-family` declared in CSS rather than whatever family name
/// the TTF's own `name` table reports.
///
/// All fields are `Option` because each descriptor is independently optional
/// at the CSS level. Missing fields fall back to the values parley reads
/// from the font's own metadata.
#[derive(Clone, Debug, Default)]
pub struct FontFaceOverrides {
    /// `font-family` descriptor (the alias the rest of the stylesheet uses).
    pub family_name: Option<String>,
    /// `font-weight` descriptor range.
    pub weight: Option<parley::fontique::FontWeightRange>,
    /// `font-style` descriptor range.
    pub style: Option<parley::fontique::FontStyleRange>,
    /// `font-stretch` (`font-width`) descriptor range.
    pub width: Option<parley::fontique::FontWidthRange>,
    /// Codepoints for which the face is eligible.
    pub unicode_range: Option<parley::fontique::UnicodeRangeSet>,
}

impl FontFaceOverrides {
    pub(crate) fn as_font_info(&self) -> parley::fontique::FontInfoOverride<'_> {
        parley::fontique::FontInfoOverride {
            family_name: self.family_name.as_deref(),
            weight: self.weight,
            style: self.style,
            width: self.width,
            unicode_range: self.unicode_range.as_ref(),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FontFaceDeclaration {
    pub url: Url,
    pub format: FontFaceSourceFormatKeyword,
    pub overrides: FontFaceOverrides,
}

#[derive(Clone, Debug)]
pub enum Resource {
    Image(ImageType, u32, u32, Arc<Vec<u8>>),
    #[cfg(feature = "svg")]
    Svg(ImageType, crate::node::SvgImageData),
    Css(DocumentStyleSheet),
    /// Stylesheet loaded for an `@import` rule, to be attached to the rule on the document thread
    ImportedCss(ServoArc<Locked<ImportRule>>, ServoArc<Stylesheet>),
    Font(Bytes, FontFaceOverrides),
    /// Completion of a deferred font source, including a failed load.
    FontFaceLoaded(Url, Option<Bytes>),
    /// HTML fetched for an `<iframe>` element's `src`
    DocumentSrc(String),
    None,
}

pub(crate) struct ResourceHandler<T: Send + Sync + 'static> {
    doc_id: usize,
    request_id: usize,
    node_id: Option<NodeId>,
    tx: Sender<DocumentEvent>,
    shell_provider: Arc<dyn ShellProvider>,
    data: T,
}

impl<T: Send + Sync + 'static> ResourceHandler<T> {
    pub(crate) fn new(
        tx: Sender<DocumentEvent>,
        doc_id: usize,
        node_id: Option<NodeId>,
        shell_provider: Arc<dyn ShellProvider>,
        data: T,
    ) -> Self {
        static REQUEST_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
        Self {
            request_id: REQUEST_ID_COUNTER.fetch_add(1, Ao::Relaxed),
            doc_id,
            node_id,
            tx,
            shell_provider,
            data,
        }
    }

    pub(crate) fn boxed(
        tx: Sender<DocumentEvent>,
        doc_id: usize,
        node_id: Option<NodeId>,
        shell_provider: Arc<dyn ShellProvider>,
        data: T,
    ) -> Box<dyn NetHandler>
    where
        ResourceHandler<T>: NetHandler,
    {
        Box::new(Self::new(tx, doc_id, node_id, shell_provider, data)) as _
    }

    pub(crate) fn request_id(&self) -> usize {
        self.request_id
    }

    fn respond(&self, resolved_url: String, result: Result<Resource, String>) {
        let response = ResourceLoadResponse {
            request_id: self.request_id,
            node_id: self.node_id,
            resolved_url: Some(resolved_url),
            result,
        };
        let _ = self.tx.send(DocumentEvent::ResourceLoad(response));
        self.shell_provider.request_redraw();
    }
}

#[allow(unused)]
pub struct ResourceLoadResponse {
    pub request_id: usize,
    pub node_id: Option<NodeId>,
    pub resolved_url: Option<String>,
    pub result: Result<Resource, String>,
}

pub struct StylesheetHandler {
    pub source_url: Url,
    pub guard: SharedRwLock,
    pub net_provider: Arc<dyn NetProvider>,
    pub abort_signal: Option<AbortSignal>,
}

impl NetHandler for ResourceHandler<StylesheetHandler> {
    fn error(self: Box<Self>, url: String, error: String) {
        self.respond(url, Err(error));
    }

    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        // NOTE(Nico): I don't *think* external stylesheets should have HTML entities escaped
        // let escaped_css = html_escape::decode_html_entities(css);

        let sheet = Stylesheet::from_bytes(
            &bytes,
            self.data.source_url.clone().into(),
            None,
            None,
            Origin::Author,
            ServoArc::new(self.data.guard.wrap(MediaList::empty())),
            self.data.guard.clone(),
            Some(&StylesheetLoader {
                tx: self.tx.clone(),
                doc_id: self.doc_id,
                net_provider: self.data.net_provider.clone(),
                shell_provider: self.shell_provider.clone(),
                abort_signal: self.data.abort_signal.clone(),
                import_depth: 0,
            }),
            None, // error_reporter
            QuirksMode::NoQuirks,
        );

        self.respond(
            resolved_url,
            Ok(Resource::Css(DocumentStyleSheet(ServoArc::new(sheet)))),
        );
    }
}

/// Maximum depth of nested `@import` rules. Imports nested deeper than this
/// are refused, preventing unbounded recursion (e.g. a stylesheet whose
/// resolved `@import` URL yields another stylesheet importing in turn, with
/// the URL growing geometrically at each level).
const MAX_IMPORT_DEPTH: usize = 16;

#[derive(Clone)]
pub(crate) struct StylesheetLoader {
    pub(crate) tx: Sender<DocumentEvent>,
    pub(crate) doc_id: usize,
    pub(crate) net_provider: Arc<dyn NetProvider>,
    pub(crate) shell_provider: Arc<dyn ShellProvider>,
    pub(crate) abort_signal: Option<AbortSignal>,
    /// Depth of `@import` nesting for the stylesheet this loader is parsing
    /// (0 for a top-level stylesheet)
    pub(crate) import_depth: usize,
}
impl ServoStylesheetLoader for StylesheetLoader {
    fn request_stylesheet(
        &self,
        url: CssUrl,
        location: SourceLocation,
        lock: &SharedRwLock,
        media: ServoArc<Locked<MediaList>>,
        supports: Option<ImportSupportsCondition>,
        layer: ImportLayer,
    ) -> ServoArc<Locked<ImportRule>> {
        if self.import_depth >= MAX_IMPORT_DEPTH || !supports.as_ref().is_none_or(|s| s.enabled) {
            return ServoArc::new(lock.wrap(ImportRule {
                url,
                stylesheet: ImportSheet::new_refused(),
                supports,
                layer,
                source_location: location,
            }));
        }

        let import = ImportRule {
            url,
            stylesheet: ImportSheet::new_pending(),
            supports,
            layer,
            source_location: location,
        };

        let url = import.url.url().unwrap().clone();
        let import = ServoArc::new(lock.wrap(import));
        self.net_provider.fetch(
            self.doc_id,
            stamped_request(url.as_ref().clone(), self.abort_signal.as_ref()),
            ResourceHandler::boxed(
                self.tx.clone(),
                self.doc_id,
                None, // node_id
                self.shell_provider.clone(),
                NestedStylesheetHandler {
                    url: url.clone(),
                    loader: StylesheetLoader {
                        import_depth: self.import_depth + 1,
                        ..self.clone()
                    },
                    lock: lock.clone(),
                    media,
                    import_rule: import.clone(),
                },
            ),
        );

        import
    }
}

struct NestedStylesheetHandler {
    loader: StylesheetLoader,
    lock: SharedRwLock,
    url: ServoArc<Url>,
    media: ServoArc<Locked<MediaList>>,
    import_rule: ServoArc<Locked<ImportRule>>,
}

impl NetHandler for ResourceHandler<NestedStylesheetHandler> {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        // NOTE(Nico): I don't *think* external stylesheets should have HTML entities escaped
        // let escaped_css = html_escape::decode_html_entities(css);

        let sheet = ServoArc::new(Stylesheet::from_bytes(
            &bytes,
            UrlExtraData(self.data.url.clone()),
            None,
            None,
            Origin::Author,
            self.data.media.clone(),
            self.data.lock.clone(),
            Some(&self.data.loader),
            None, // error_reporter
            QuirksMode::NoQuirks,
        ));

        let import_rule = self.data.import_rule.clone();
        self.respond(resolved_url, Ok(Resource::ImportedCss(import_rule, sheet)))
    }
}

pub(crate) struct FontFaceHandler {
    pub format: FontFaceSourceFormatKeyword,
    pub url: Url,
}
impl NetHandler for ResourceHandler<FontFaceHandler> {
    fn bytes(mut self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let bytes = self.data.parse(bytes);
        self.respond(
            resolved_url,
            Ok(Resource::FontFaceLoaded(self.data.url.clone(), bytes)),
        )
    }

    fn error(self: Box<Self>, resolved_url: String, _error: String) {
        self.respond(
            resolved_url,
            Ok(Resource::FontFaceLoaded(self.data.url.clone(), None)),
        );
    }
}
impl FontFaceHandler {
    fn parse(&mut self, bytes: Bytes) -> Option<Bytes> {
        if self.format == FontFaceSourceFormatKeyword::None && bytes.len() >= 4 {
            self.format = match &bytes.as_ref()[0..4] {
                // WOFF (v1) files begin with 0x774F4646 ('wOFF' in ascii)
                // See: <https://w3c.github.io/woff/woff1/spec/Overview.html#WOFFHeader>
                #[cfg(feature = "woff")]
                b"wOFF" => FontFaceSourceFormatKeyword::Woff,
                // WOFF2 files begin with 0x774F4632 ('wOF2' in ascii)
                // See: <https://w3c.github.io/woff/woff2/#woff20Header>
                #[cfg(feature = "woff")]
                b"wOF2" => FontFaceSourceFormatKeyword::Woff2,
                // Opentype fonts with CFF data begin with 0x4F54544F ('OTTO' in ascii)
                // See: <https://learn.microsoft.com/en-us/typography/opentype/spec/otff#organization-of-an-opentype-font>
                b"OTTO" => FontFaceSourceFormatKeyword::Opentype,
                // Opentype fonts truetype outlines begin with 0x00010000
                // See: <https://learn.microsoft.com/en-us/typography/opentype/spec/otff#organization-of-an-opentype-font>
                &[0x00, 0x01, 0x00, 0x00] => FontFaceSourceFormatKeyword::Truetype,
                // Truetype fonts begin with 0x74727565 ('true' in ascii)
                // See: <https://developer.apple.com/fonts/TrueType-Reference-Manual/RM06/Chap6.html#ScalerTypeNote>
                b"true" => FontFaceSourceFormatKeyword::Truetype,
                _ => FontFaceSourceFormatKeyword::None,
            }
        }

        // Satisfy rustc's mutability linting with woff feature both enabled/disabled
        #[cfg(feature = "woff")]
        let mut bytes = bytes;

        match self.format {
            #[cfg(feature = "woff")]
            FontFaceSourceFormatKeyword::Woff => {
                #[cfg(feature = "tracing")]
                tracing::info!("Decompressing woff1 font");

                // Use wuff crate to decompress font
                bytes = Bytes::from(wuff::decompress_woff1(&bytes).ok()?);
            }
            #[cfg(feature = "woff")]
            FontFaceSourceFormatKeyword::Woff2 => {
                #[cfg(feature = "tracing")]
                tracing::info!("Decompressing woff2 font");

                // Use wuff crate to decompress font
                bytes = Bytes::from(wuff::decompress_woff2(&bytes).ok()?);
            }
            FontFaceSourceFormatKeyword::None => {
                // Should this be an error?
                return None;
            }
            _ => {}
        }

        Some(bytes)
    }
}

/// Collect the font faces declared in a stylesheet.
pub(crate) fn collect_font_faces(
    sheet: &Stylesheet,
    read_guard: &SharedRwLockReadGuard,
) -> Vec<FontFaceDeclaration> {
    collect_font_face_rules(
        sheet.contents(read_guard).rules(read_guard).iter(),
        read_guard,
    )
}

/// Collect the font faces among `rules`.
pub(crate) fn collect_font_face_rules<'a>(
    rules: impl Iterator<Item = &'a CssRule>,
    read_guard: &SharedRwLockReadGuard,
) -> Vec<FontFaceDeclaration> {
    rules
        .filter_map(|rule| match rule {
            CssRule::FontFace(font_face) => {
                let descriptor = &font_face.read_with(read_guard).descriptors;
                let family = descriptor.font_family.as_ref()?;
                let src = descriptor.src.as_ref()?;
                // Capture the @font-face descriptors so parley can register
                // the font under the CSS-declared family name (and weight /
                // style) rather than whatever metadata the TTF reports.
                let overrides = FontFaceOverrides {
                    family_name: Some(family.name.to_string()),
                    weight: descriptor.font_weight.as_ref().and_then(|range| {
                        let range = range.compute()?;
                        Some(parley::fontique::FontWeightRange::new(
                            parley::fontique::FontWeight::new(range.0.value()),
                            parley::fontique::FontWeight::new(range.1.value()),
                        ))
                    }),
                    style: descriptor.font_style.as_ref().map(stylo_to_fontique_style),
                    width: descriptor.font_width.as_ref().and_then(|range| {
                        let range = range.compute()?;
                        Some(parley::fontique::FontWidthRange::new(
                            parley::fontique::FontWidth::from_percentage(range.0.0.to_float()),
                            parley::fontique::FontWidth::from_percentage(range.1.0.to_float()),
                        ))
                    }),
                    unicode_range: descriptor.unicode_range.as_ref().map(|ranges| {
                        parley::fontique::UnicodeRangeSet::new(
                            ranges.iter().map(|range| range.start..=range.end),
                        )
                    }),
                };
                Some((src, overrides))
            }
            _ => None,
        })
        .filter_map(|(source_list, overrides)| {
            let (url, format) = preferred_font_source(source_list)?;
            Some(FontFaceDeclaration {
                url,
                format,
                overrides,
            })
        })
        .collect()
}

/// The first source in an `@font-face` `src` list that is in a format we
/// support, with its resolved url and format.
fn preferred_font_source(source_list: &SourceList) -> Option<(Url, FontFaceSourceFormatKeyword)> {
    source_list.0.iter().find_map(|source| {
        let url_source = match source {
            Source::Url(url_source) => url_source,
            // TODO: support local fonts in @font-face
            Source::Local(_) => return None,
        };

        // A relative url with no base url to resolve against
        // yields None; skip the source instead of panicking
        let Some(url) = url_source.url.url() else {
            #[cfg(feature = "tracing")]
            tracing::warn!("Skipping @font-face source with unresolvable url");
            return None;
        };

        let format = match &url_source.format_hint {
            Some(FontFaceSourceFormat::Keyword(keyword)) => *keyword,
            Some(FontFaceSourceFormat::String(string)) => font_format_from_string(string),
            None => font_format_from_extension(url),
        };

        let supported = match format {
            FontFaceSourceFormatKeyword::None
            | FontFaceSourceFormatKeyword::Collection
            | FontFaceSourceFormatKeyword::Opentype
            | FontFaceSourceFormatKeyword::Truetype => true,
            FontFaceSourceFormatKeyword::Woff | FontFaceSourceFormatKeyword::Woff2 => {
                cfg!(feature = "woff")
            }
            FontFaceSourceFormatKeyword::EmbeddedOpentype
            | FontFaceSourceFormatKeyword::Svg
            | FontFaceSourceFormatKeyword::Unknown => false,
        };
        if !supported {
            #[cfg(feature = "tracing")]
            tracing::warn!("Skipping unsupported font of type {:?}", format);
            return None;
        }

        Some((url.as_ref().clone(), format))
    })
}

/// Map a `format("...")` string hint to a format. Besides the keywords, this
/// accepts the legacy `-variations` strings and the `ttf`/`otf` extensions.
fn font_format_from_string(format: &str) -> FontFaceSourceFormatKeyword {
    const FORMATS: &[(&str, FontFaceSourceFormatKeyword)] = &[
        ("collection", FontFaceSourceFormatKeyword::Collection),
        (
            "embedded-opentype",
            FontFaceSourceFormatKeyword::EmbeddedOpentype,
        ),
        ("opentype", FontFaceSourceFormatKeyword::Opentype),
        ("opentype-variations", FontFaceSourceFormatKeyword::Opentype),
        ("otf", FontFaceSourceFormatKeyword::Opentype),
        ("svg", FontFaceSourceFormatKeyword::Svg),
        ("truetype", FontFaceSourceFormatKeyword::Truetype),
        ("truetype-variations", FontFaceSourceFormatKeyword::Truetype),
        ("ttf", FontFaceSourceFormatKeyword::Truetype),
        ("woff", FontFaceSourceFormatKeyword::Woff),
        ("woff-variations", FontFaceSourceFormatKeyword::Woff),
        ("woff2", FontFaceSourceFormatKeyword::Woff2),
        ("woff2-variations", FontFaceSourceFormatKeyword::Woff2),
    ];
    FORMATS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(format))
        .map_or(FontFaceSourceFormatKeyword::Unknown, |(_, format)| *format)
}

/// Guess a font's format from the file extension of its url's path. Urls
/// with no known extension, such as `data:` urls, get
/// `FontFaceSourceFormatKeyword::None` and are sniffed from their bytes once
/// fetched.
fn font_format_from_extension(url: &Url) -> FontFaceSourceFormatKeyword {
    const EXTENSIONS: &[(&str, FontFaceSourceFormatKeyword)] = &[
        ("eot", FontFaceSourceFormatKeyword::EmbeddedOpentype),
        ("otf", FontFaceSourceFormatKeyword::Opentype),
        ("svg", FontFaceSourceFormatKeyword::Svg),
        ("ttc", FontFaceSourceFormatKeyword::Collection),
        ("ttf", FontFaceSourceFormatKeyword::Truetype),
        ("woff", FontFaceSourceFormatKeyword::Woff),
        ("woff2", FontFaceSourceFormatKeyword::Woff2),
    ];
    if url.cannot_be_a_base() {
        return FontFaceSourceFormatKeyword::None;
    }
    let file_name = url.path().rsplit('/').next().unwrap_or_default();
    let Some((_, extension)) = file_name.rsplit_once('.') else {
        return FontFaceSourceFormatKeyword::None;
    };
    EXTENSIONS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(extension))
        .map_or(FontFaceSourceFormatKeyword::None, |(_, format)| *format)
}

/// Translate stylo's `@font-face` `font-style` descriptor into fontique's
/// `FontStyleRange`. Both parse CSS `normal` as `oblique 0deg 0deg`.
fn stylo_to_fontique_style(style: &FontStyleRange) -> parley::fontique::FontStyleRange {
    use parley::fontique::FontStyleRange as Fq;
    match style {
        FontStyleRange::Italic => Fq::Italic,
        FontStyleRange::Oblique(min, max) => {
            let default = Fq::DEFAULT_OBLIQUE_ANGLE;
            Fq::oblique(
                min.degrees().unwrap_or(default),
                max.degrees().unwrap_or(default),
            )
        }
    }
}

/// Handles HTML fetched for an `<iframe>` element's `src`
pub(crate) struct DocumentSrcHandler;

impl NetHandler for ResourceHandler<DocumentSrcHandler> {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let html = String::from_utf8_lossy(&bytes).into_owned();
        self.respond(resolved_url, Ok(Resource::DocumentSrc(html)));
    }
}

pub struct ImageHandler {
    kind: ImageType,
}
impl ImageHandler {
    pub fn new(kind: ImageType) -> Self {
        Self { kind }
    }
}

impl NetHandler for ResourceHandler<ImageHandler> {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let result = self.data.parse(bytes);
        self.respond(resolved_url, result)
    }
}

impl ImageHandler {
    fn parse(&self, bytes: Bytes) -> Result<Resource, String> {
        let image_err = match image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .expect("IO errors impossible with Cursor")
            .decode()
        {
            Ok(image) => {
                let width = image.width();
                let height = image.height();
                let raw_rgba8_data = image.into_rgba8().into_raw();
                return Ok(Resource::Image(
                    self.kind,
                    width,
                    height,
                    Arc::new(raw_rgba8_data),
                ));
            }
            Err(e) => e.to_string(),
        };

        #[cfg(feature = "svg")]
        let svg_err = {
            use crate::util::parse_svg_image;
            match parse_svg_image(&bytes) {
                Ok(svg) => return Ok(Resource::Svg(self.kind, svg)),
                Err(e) => e.to_string(),
            }
        };
        #[cfg(not(feature = "svg"))]
        let svg_err = "svg feature disabled";

        Err(format!(
            "Could not parse image ({} bytes): image-crate error: {image_err}; svg fallback error: {svg_err}",
            bytes.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley::fontique::FontStyleRange as Fq;
    use style::stylesheets::AllowImportRules;
    use style::values::specified::Angle;

    fn oblique(min_deg: f32, max_deg: f32) -> FontStyleRange {
        FontStyleRange::Oblique(Angle::from_degrees(min_deg), Angle::from_degrees(max_deg))
    }

    /// The source picked from the `src` list of each `@font-face` rule in `css`
    fn preferred_sources(css: &str) -> Vec<Option<(String, FontFaceSourceFormatKeyword)>> {
        let lock = SharedRwLock::new();
        let base_url = Url::parse("https://example.com/css/style.css").unwrap();
        let sheet = Stylesheet::from_str(
            css,
            UrlExtraData(ServoArc::new(base_url)),
            Origin::Author,
            ServoArc::new(lock.wrap(MediaList::empty())),
            lock.clone(),
            None,
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::No,
        );
        let guard = lock.read();
        sheet
            .contents(&guard)
            .rules(&guard)
            .iter()
            .filter_map(|rule| match rule {
                CssRule::FontFace(font_face) => font_face.read_with(&guard).descriptors.src.clone(),
                _ => None,
            })
            .map(|src| preferred_font_source(&src).map(|(url, format)| (url.to_string(), format)))
            .collect()
    }

    #[test]
    fn bootstrap_glyphicons_skip_eot_source() {
        let sources = preferred_sources(
            "@font-face {
                font-family: 'Glyphicons Halflings';
                src: url('../fonts/glyphicons-halflings-regular.eot');
                src: url('../fonts/glyphicons-halflings-regular.eot?#iefix') format('embedded-opentype'),
                    url('../fonts/glyphicons-halflings-regular.woff2') format('woff2'),
                    url('../fonts/glyphicons-halflings-regular.woff') format('woff'),
                    url('../fonts/glyphicons-halflings-regular.ttf') format('truetype'),
                    url('../fonts/glyphicons-halflings-regular.svg#glyphicons_halflingsregular') format('svg');
            }",
        );
        let expected = if cfg!(feature = "woff") {
            (
                "https://example.com/fonts/glyphicons-halflings-regular.woff2",
                FontFaceSourceFormatKeyword::Woff2,
            )
        } else {
            (
                "https://example.com/fonts/glyphicons-halflings-regular.ttf",
                FontFaceSourceFormatKeyword::Truetype,
            )
        };
        assert_eq!(sources, [Some((expected.0.to_string(), expected.1))]);
    }

    #[test]
    fn format_strings_map_to_formats() {
        use FontFaceSourceFormatKeyword as Kw;
        for (string, format) in [
            ("collection", Kw::Collection),
            ("embedded-opentype", Kw::EmbeddedOpentype),
            ("opentype", Kw::Opentype),
            ("OpenType", Kw::Opentype),
            ("opentype-variations", Kw::Opentype),
            ("otf", Kw::Opentype),
            ("svg", Kw::Svg),
            ("truetype", Kw::Truetype),
            ("truetype-variations", Kw::Truetype),
            ("ttf", Kw::Truetype),
            ("woff", Kw::Woff),
            ("woff-variations", Kw::Woff),
            ("woff2", Kw::Woff2),
            ("woff2-variations", Kw::Woff2),
            ("x-unknown", Kw::Unknown),
        ] {
            assert_eq!(font_format_from_string(string), format, "{string}");
        }
    }

    #[test]
    fn extension_ignores_query_and_fragment() {
        use FontFaceSourceFormatKeyword as Kw;
        for (url, format) in [
            ("https://example.com/a.eot?#iefix", Kw::EmbeddedOpentype),
            ("https://example.com/a.svg#font", Kw::Svg),
            ("https://example.com/a.TTF?v=4.7.0", Kw::Truetype),
            ("https://example.com/a.otf", Kw::Opentype),
            ("https://example.com/a.woff2?v=1#x", Kw::Woff2),
            ("https://example.com/fonts.v2/font", Kw::None),
            ("https://example.com/font?name=a.ttf", Kw::None),
            ("data:font/ttf;base64,AAEAAA.ttf", Kw::None),
        ] {
            let url = Url::parse(url).unwrap();
            assert_eq!(font_format_from_extension(&url), format, "{url}");
        }
    }

    #[test]
    fn unsupported_sources_are_skipped() {
        use FontFaceSourceFormatKeyword as Kw;
        let sources = preferred_sources(
            "@font-face { font-family: a; src: url(a.ttf) format('x-unknown'), url(a.otf); }
            @font-face { font-family: b; src: url(b.svg#b), url(b.eot), url(b.ttf?v=1#x); }
            @font-face { font-family: c; src: url(c.eot) format(embedded-opentype); }
            @font-face { font-family: d; src: url(d.eot) format(truetype); }",
        );
        assert_eq!(
            sources,
            [
                Some(("https://example.com/css/a.otf".to_string(), Kw::Opentype)),
                Some((
                    "https://example.com/css/b.ttf?v=1#x".to_string(),
                    Kw::Truetype
                )),
                None,
                Some(("https://example.com/css/d.eot".to_string(), Kw::Truetype)),
            ]
        );
    }

    #[test]
    fn italic_maps_to_italic() {
        assert_eq!(stylo_to_fontique_style(&FontStyleRange::Italic), Fq::Italic,);
    }

    #[test]
    fn oblique_zero_zero_maps_to_normal() {
        // Stylo parses bare CSS `normal` as `Oblique(0deg, 0deg)`.
        assert_eq!(stylo_to_fontique_style(&oblique(0.0, 0.0)), Fq::NORMAL);
    }

    #[test]
    fn oblique_single_angle_maps_to_oblique_with_min() {
        assert_eq!(
            stylo_to_fontique_style(&oblique(14.0, 14.0)),
            Fq::oblique(14.0, 14.0),
        );
    }

    #[test]
    fn oblique_range_keeps_both_angles() {
        assert_eq!(
            stylo_to_fontique_style(&oblique(10.0, 20.0)),
            Fq::oblique(10.0, 20.0),
        );
    }
}
