use image::DynamicImage;
use selectors::context::QuirksMode;
use std::{
    io::Cursor,
    str::FromStr,
    sync::atomic::AtomicBool,
    sync::{Arc, OnceLock},
};
use style::{
    font_face::{FontFaceSourceFormat, FontFaceSourceFormatKeyword, Source},
    media_queries::MediaList,
    parser::ParserContext,
    servo_arc::Arc as ServoArc,
    shared_lock::SharedRwLock,
    shared_lock::{Locked, SharedRwLockReadGuard},
    stylesheets::{
        import_rule::{ImportLayer, ImportSheet, ImportSupportsCondition},
        AllowImportRules, CssRule, CssRules, DocumentStyleSheet, ImportRule, Origin, Stylesheet,
        StylesheetContents, StylesheetInDocument, StylesheetLoader as ServoStylesheetLoader,
        UrlExtraData,
    },
    values::{CssUrl, SourceLocation},
};

use blitz_traits::net::{Bytes, RequestHandler, SharedCallback, SharedProvider};

use url::Url;
use usvg::Tree;

static FONT_DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();

#[derive(Clone, Debug)]
pub enum Resource {
    Image(usize, Arc<DynamicImage>),
    Svg(usize, Box<Tree>),
    Css(usize, DocumentStyleSheet),
    Font(Bytes),
}
pub(crate) struct CssHandler {
    pub node: usize,
    pub source_url: Url,
    pub guard: SharedRwLock,
    pub provider: SharedProvider,
    pub callback: SharedCallback<Resource>,
}

#[derive(Clone)]
struct StylesheetLoader(SharedCallback<Resource>, SharedProvider);
impl ServoStylesheetLoader for StylesheetLoader {
    fn request_stylesheet(
        &self,
        url: CssUrl,
        location: SourceLocation,
        context: &ParserContext,
        lock: &SharedRwLock,
        media: ServoArc<Locked<MediaList>>,
        supports: Option<ImportSupportsCondition>,
        layer: ImportLayer,
    ) -> ServoArc<Locked<ImportRule>> {
        if !supports.as_ref().map_or(true, |s| s.enabled) {
            return ServoArc::new(lock.wrap(ImportRule {
                url,
                stylesheet: ImportSheet::new_refused(),
                supports,
                layer,
                source_location: location,
            }));
        }

        let sheet = ServoArc::new(Stylesheet {
            contents: StylesheetContents::from_data(
                CssRules::new(Vec::new(), lock),
                context.stylesheet_origin,
                context.url_data.clone(),
                context.quirks_mode,
            ),
            media,
            shared_lock: lock.clone(),
            disabled: AtomicBool::new(false),
        });

        let stylesheet = ImportSheet::new(sheet.clone());
        let import = ImportRule {
            url,
            stylesheet,
            supports,
            layer,
            source_location: location,
        };

        let url = import.url.url().unwrap().clone();
        let this = self.clone();
        let read_lock = lock.clone();

        self.1.fetch(
            url.as_ref().clone(),
            Box::new(move |bytes: Bytes| {
                let css = std::str::from_utf8(&bytes).expect("Invalid UTF8");
                let escaped_css = html_escape::decode_html_entities(css);
                Stylesheet::update_from_str(
                    sheet.as_ref(),
                    &escaped_css,
                    UrlExtraData(url),
                    Some(&this),
                    None,
                    AllowImportRules::Yes,
                );
                fetch_font_face(sheet.as_ref(), &this.1, &this.0, &read_lock.read())
            }),
        );

        ServoArc::new(lock.wrap(import))
    }
}
impl RequestHandler for CssHandler {
    fn bytes(self: Box<Self>, bytes: Bytes) {
        let css = std::str::from_utf8(&bytes).expect("Invalid UTF8");
        let escaped_css = html_escape::decode_html_entities(css);
        let sheet = Stylesheet::from_str(
            &escaped_css,
            self.source_url.into(),
            Origin::Author,
            ServoArc::new(self.guard.wrap(MediaList::empty())),
            self.guard.clone(),
            Some(&StylesheetLoader(
                self.callback.clone(),
                self.provider.clone(),
            )),
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );
        let read_guard = self.guard.read();
        fetch_font_face(&sheet, &self.provider, &self.callback, &read_guard);

        self.callback.call(Resource::Css(
            self.node,
            DocumentStyleSheet(ServoArc::new(sheet)),
        ))
    }
}
struct FontFaceHandler(FontFaceSourceFormatKeyword, SharedCallback<Resource>);
impl RequestHandler for FontFaceHandler {
    fn bytes(mut self: Box<Self>, mut bytes: Bytes) {
        if self.0 == FontFaceSourceFormatKeyword::None {
            self.0 = match bytes.as_ref() {
                // https://w3c.github.io/woff/woff2/#woff20Header
                [0x77, 0x4F, 0x46, 0x32, ..] => FontFaceSourceFormatKeyword::Woff2,
                // https://learn.microsoft.com/en-us/typography/opentype/spec/otff#organization-of-an-opentype-font
                [0x4F, 0x54, 0x54, 0x4F, ..] => FontFaceSourceFormatKeyword::Opentype,
                // https://developer.apple.com/fonts/TrueType-Reference-Manual/RM06/Chap6.html#ScalerTypeNote
                [0x00, 0x01, 0x00, 0x00, ..] | [0x74, 0x72, 0x75, 0x65, ..] => {
                    FontFaceSourceFormatKeyword::Truetype
                }
                _ => FontFaceSourceFormatKeyword::None,
            }
        }
        match self.0 {
            FontFaceSourceFormatKeyword::Woff2 => {
                tracing::info!("Decompressing woff2 font");
                let decompressed = woff::version2::decompress(&bytes);
                if let Some(decompressed) = decompressed {
                    bytes = Bytes::from(decompressed);
                } else {
                    tracing::warn!("Failed to decompress woff2 font");
                }
            }
            FontFaceSourceFormatKeyword::None => {
                return;
            }
            _ => {}
        }

        self.1.call(Resource::Font(bytes))
    }
}

fn fetch_font_face(
    sheet: &Stylesheet,
    network_provider: &SharedProvider,
    resource_callback: &SharedCallback<Resource>,
    read_guard: &SharedRwLockReadGuard,
) {
    sheet
        .rules(read_guard)
        .iter()
        .filter_map(|rule| match rule {
            CssRule::FontFace(font_face) => font_face.read_with(read_guard).sources.as_ref(),
            _ => None,
        })
        .flat_map(|source_list| &source_list.0)
        .filter_map(|source| match source {
            Source::Url(url_source) => Some(url_source),
            _ => None,
        })
        .for_each(|url_source| {
            let mut format = match &url_source.format_hint {
                Some(FontFaceSourceFormat::Keyword(fmt)) => *fmt,
                Some(FontFaceSourceFormat::String(str)) => match str.as_str() {
                    "woff2" => FontFaceSourceFormatKeyword::Woff2,
                    "ttf" => FontFaceSourceFormatKeyword::Truetype,
                    "otf" => FontFaceSourceFormatKeyword::Opentype,
                    _ => FontFaceSourceFormatKeyword::None,
                },
                _ => FontFaceSourceFormatKeyword::None,
            };
            if format == FontFaceSourceFormatKeyword::None {
                let Some((_, end)) = url_source.url.as_str().rsplit_once('.') else {
                    return;
                };
                format = match end {
                    "woff2" => FontFaceSourceFormatKeyword::Woff2,
                    "woff" => FontFaceSourceFormatKeyword::Woff,
                    "ttf" => FontFaceSourceFormatKeyword::Truetype,
                    "otf" => FontFaceSourceFormatKeyword::Opentype,
                    "svg" => FontFaceSourceFormatKeyword::Svg,
                    "eot" => FontFaceSourceFormatKeyword::EmbeddedOpentype,
                    _ => FontFaceSourceFormatKeyword::None,
                }
            }
            if let font_format @ (FontFaceSourceFormatKeyword::Svg
            | FontFaceSourceFormatKeyword::EmbeddedOpentype
            | FontFaceSourceFormatKeyword::Woff) = format
            {
                tracing::warn!("Skipping unsupported font of type {:?}", font_format);
                return;
            }
            network_provider.fetch(
                Url::from_str(url_source.url.as_str()).unwrap(),
                Box::new(FontFaceHandler(format, resource_callback.clone())),
            )
        });
}

pub(crate) struct ImageHandler(pub usize, pub SharedCallback<Resource>);
impl RequestHandler for ImageHandler {
    fn bytes(self: Box<Self>, bytes: Bytes) {
        // Try parse image
        if let Ok(image) = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .expect("IO errors impossible with Cursor")
            .decode()
        {
            self.1.call(Resource::Image(self.0, Arc::new(image)));
            return;
        };
        // Try parse SVG

        // TODO: Use fontique
        let fontdb = FONT_DB.get_or_init(|| {
            let mut fontdb = usvg::fontdb::Database::new();
            fontdb.load_system_fonts();
            Arc::new(fontdb)
        });

        let options = usvg::Options {
            fontdb: fontdb.clone(),
            ..Default::default()
        };

        const DUMMY_SVG : &[u8] = r#"<?xml version="1.0" encoding="UTF-8"?><svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#.as_bytes();

        let tree = Tree::from_data(&bytes, &options)
            .unwrap_or_else(|_| Tree::from_data(DUMMY_SVG, &options).unwrap());
        self.1.call(Resource::Svg(self.0, Box::new(tree)));
    }
}
