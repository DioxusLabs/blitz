use selectors::context::QuirksMode;
use std::{io::Cursor, sync::Arc, sync::atomic::AtomicBool};
use style::{
    font_face::{FontFaceSourceFormat, FontFaceSourceFormatKeyword, Source},
    media_queries::MediaList,
    parser::ParserContext,
    servo_arc::Arc as ServoArc,
    shared_lock::SharedRwLock,
    shared_lock::{Locked, SharedRwLockReadGuard},
    stylesheets::{
        AllowImportRules, CssRule, CssRules, DocumentStyleSheet, ImportRule, Origin, Stylesheet,
        StylesheetContents, StylesheetInDocument, StylesheetLoader as ServoStylesheetLoader,
        UrlExtraData,
        import_rule::{ImportLayer, ImportSheet, ImportSupportsCondition},
    },
    values::{CssUrl, SourceLocation},
};

use blitz_traits::net::{Bytes, NetHandler, Request, SharedCallback, SharedProvider};

use url::Url;

use crate::util::ImageType;

#[derive(Clone, Debug)]
pub enum Resource {
    Image(usize, ImageType, u32, u32, Arc<Vec<u8>>),
    #[cfg(feature = "svg")]
    Svg(usize, ImageType, Box<usvg::Tree>),
    Css(usize, DocumentStyleSheet),
    Font(Bytes),
    None,
}
pub struct CssHandler {
    pub node: usize,
    pub source_url: Url,
    pub guard: SharedRwLock,
    pub provider: SharedProvider<Resource>,
}

#[derive(Clone)]
pub(crate) struct StylesheetLoader(pub(crate) usize, pub(crate) SharedProvider<Resource>);
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
        if !supports.as_ref().is_none_or(|s| s.enabled) {
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

        struct StylesheetLoaderInner {
            loader: StylesheetLoader,
            read_lock: SharedRwLock,
            url: ServoArc<Url>,
            sheet: ServoArc<Stylesheet>,
            provider: SharedProvider<Resource>,
        }
        impl NetHandler for StylesheetLoaderInner {
            type Data = Resource;
            fn bytes(
                self: Box<Self>,
                doc_id: usize,
                bytes: Bytes,
                callback: SharedCallback<Self::Data>,
            ) {
                let Ok(css) = std::str::from_utf8(&bytes) else {
                    callback.call(doc_id, Err(Some(String::from("Invalid UTF8"))));
                    return;
                };
                let escaped_css = html_escape::decode_html_entities(css);
                Stylesheet::update_from_str(
                    &self.sheet,
                    &escaped_css,
                    UrlExtraData(self.url),
                    Some(&self.loader),
                    None,
                    AllowImportRules::Yes,
                );
                fetch_font_face(doc_id, &self.sheet, &self.provider, &self.read_lock.read());
                callback.call(doc_id, Ok(Resource::None))
            }
        }
        let url = import.url.url().unwrap();
        self.1.fetch(
            self.0,
            Request::get(url.as_ref().clone()),
            Box::new(StylesheetLoaderInner {
                url: url.clone(),
                loader: self.clone(),
                read_lock: lock.clone(),
                sheet: sheet.clone(),
                provider: self.1.clone(),
            }),
        );

        ServoArc::new(lock.wrap(import))
    }
}
impl NetHandler for CssHandler {
    type Data = Resource;
    fn bytes(self: Box<Self>, doc_id: usize, bytes: Bytes, callback: SharedCallback<Resource>) {
        let Ok(css) = std::str::from_utf8(&bytes) else {
            callback.call(doc_id, Err(Some(String::from("Invalid UTF8"))));
            return;
        };

        let escaped_css = html_escape::decode_html_entities(css);
        let sheet = Stylesheet::from_str(
            &escaped_css,
            self.source_url.into(),
            Origin::Author,
            ServoArc::new(self.guard.wrap(MediaList::empty())),
            self.guard.clone(),
            Some(&StylesheetLoader(doc_id, self.provider.clone())),
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );
        let read_guard = self.guard.read();
        fetch_font_face(doc_id, &sheet, &self.provider, &read_guard);

        callback.call(
            doc_id,
            Ok(Resource::Css(
                self.node,
                DocumentStyleSheet(ServoArc::new(sheet)),
            )),
        )
    }
}
struct FontFaceHandler(FontFaceSourceFormatKeyword);
impl NetHandler for FontFaceHandler {
    type Data = Resource;
    fn bytes(mut self: Box<Self>, doc_id: usize, bytes: Bytes, callback: SharedCallback<Resource>) {
        if self.0 == FontFaceSourceFormatKeyword::None {
            self.0 = match bytes.as_ref() {
                // WOFF (v1) files begin with 0x774F4646 ('wOFF' in ascii)
                // See: <https://w3c.github.io/woff/woff1/spec/Overview.html#WOFFHeader>
                // #[cfg(any(feature = "woff-c"))]
                // [b'w', b'O', b'F', b'F', ..] => FontFaceSourceFormatKeyword::Woff,
                // WOFF2 files begin with 0x774F4632 ('wOF2' in ascii)
                // See: <https://w3c.github.io/woff/woff2/#woff20Header>
                #[cfg(any(feature = "woff-c", feature = "woff-rust"))]
                [b'w', b'O', b'F', b'2', ..] => FontFaceSourceFormatKeyword::Woff2,
                // Opentype fonts with CFF data begin with 0x4F54544F ('OTTO' in ascii)
                // See: <https://learn.microsoft.com/en-us/typography/opentype/spec/otff#organization-of-an-opentype-font>
                [b'O', b'T', b'T', b'O', ..] => FontFaceSourceFormatKeyword::Opentype,
                // Opentype fonts truetype outlines begin with 0x00010000
                // See: <https://learn.microsoft.com/en-us/typography/opentype/spec/otff#organization-of-an-opentype-font>
                [0x00, 0x01, 0x00, 0x00, ..] => FontFaceSourceFormatKeyword::Truetype,
                // Truetype fonts begin with 0x74727565 ('true' in ascii)
                // See: <https://developer.apple.com/fonts/TrueType-Reference-Manual/RM06/Chap6.html#ScalerTypeNote>
                [b't', b'r', b'u', b'e', ..] => FontFaceSourceFormatKeyword::Truetype,
                _ => FontFaceSourceFormatKeyword::None,
            }
        }

        // Satisfy rustc's mutability linting with woff feature both enabled/disabled
        #[cfg(any(feature = "woff-c", feature = "woff-rust"))]
        let mut bytes = bytes;

        match self.0 {
            // #[cfg(feature = "woff-c")]
            // FontFaceSourceFormatKeyword::Woff => {
            //     #[cfg(feature = "tracing")]
            //     tracing::info!("Decompressing woff1 font");

            //     // Use woff crate to decompress font
            //     let decompressed = woff::version1::decompress(&bytes);

            //     if let Some(decompressed) = decompressed {
            //         bytes = Bytes::from(decompressed);
            //     } else {
            //         #[cfg(feature = "tracing")]
            //         tracing::warn!("Failed to decompress woff1 font");
            //     }
            // }
            #[cfg(any(feature = "woff-c", feature = "woff-rust"))]
            FontFaceSourceFormatKeyword::Woff2 => {
                #[cfg(feature = "tracing")]
                tracing::info!("Decompressing woff2 font");

                // Use woff crate to decompress font
                #[cfg(feature = "woff-c")]
                let decompressed = woff::version2::decompress(&bytes);

                // Use woff2 crate to decompress font
                #[cfg(feature = "woff-rust")]
                let decompressed = woff2::decode::convert_woff2_to_ttf(&mut bytes).ok();

                if let Some(decompressed) = decompressed {
                    bytes = Bytes::from(decompressed);
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("Failed to decompress woff2 font");
                }
            }
            FontFaceSourceFormatKeyword::None => {
                return;
            }
            _ => {}
        }

        callback.call(doc_id, Ok(Resource::Font(bytes)))
    }
}

fn fetch_font_face(
    doc_id: usize,
    sheet: &Stylesheet,
    network_provider: &SharedProvider<Resource>,
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
            if let _font_format @ (FontFaceSourceFormatKeyword::Svg
            | FontFaceSourceFormatKeyword::EmbeddedOpentype
            | FontFaceSourceFormatKeyword::Woff) = format
            {
                #[cfg(feature = "tracing")]
                tracing::warn!("Skipping unsupported font of type {:?}", _font_format);
                return;
            }
            let url = url_source.url.url().unwrap().as_ref().clone();
            network_provider.fetch(doc_id, Request::get(url), Box::new(FontFaceHandler(format)))
        });
}

pub struct ImageHandler(usize, ImageType);
impl ImageHandler {
    pub fn new(node_id: usize, kind: ImageType) -> Self {
        Self(node_id, kind)
    }
}
impl NetHandler for ImageHandler {
    type Data = Resource;
    fn bytes(self: Box<Self>, doc_id: usize, bytes: Bytes, callback: SharedCallback<Resource>) {
        // Try parse image
        if let Ok(image) = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .expect("IO errors impossible with Cursor")
            .decode()
        {
            let raw_rgba8_data = image.clone().into_rgba8().into_raw();
            callback.call(
                doc_id,
                Ok(Resource::Image(
                    self.0,
                    self.1,
                    image.width(),
                    image.height(),
                    Arc::new(raw_rgba8_data),
                )),
            );
            return;
        };

        #[cfg(feature = "svg")]
        {
            use crate::util::parse_svg;
            if let Ok(tree) = parse_svg(&bytes) {
                callback.call(doc_id, Ok(Resource::Svg(self.0, self.1, Box::new(tree))));
                return;
            }
        }

        callback.call(doc_id, Err(Some(String::from("Could not parse image"))))
    }
}
