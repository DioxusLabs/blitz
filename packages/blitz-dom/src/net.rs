use selectors::context::QuirksMode;
use std::sync::atomic::Ordering as Ao;
use std::{
    io::Cursor,
    sync::{Arc, atomic::AtomicUsize, mpsc::Sender},
};
use style::{
    font_face::{FontFaceSourceFormat, FontFaceSourceFormatKeyword, Source},
    media_queries::MediaList,
    servo_arc::Arc as ServoArc,
    shared_lock::SharedRwLock,
    shared_lock::{Locked, SharedRwLockReadGuard},
    stylesheets::{
        AllowImportRules, CssRule, DocumentStyleSheet, ImportRule, Origin, Stylesheet,
        StylesheetInDocument, StylesheetLoader as ServoStylesheetLoader, UrlExtraData,
        import_rule::{ImportLayer, ImportSheet, ImportSupportsCondition},
    },
    values::{CssUrl, SourceLocation},
};

use blitz_traits::net::{Bytes, NetHandler, Request, SharedProvider};

use url::Url;

use crate::{document::DocumentEvent, util::ImageType};

#[derive(Clone, Debug)]
pub enum Resource {
    Image(ImageType, u32, u32, Arc<Vec<u8>>),
    #[cfg(feature = "svg")]
    Svg(ImageType, Box<usvg::Tree>),
    Css(DocumentStyleSheet),
    Font(Bytes),
    None,
}

pub(crate) struct ResourceHandler<T: Send + Sync + 'static> {
    doc_id: usize,
    request_id: usize,
    node_id: Option<usize>,
    tx: Sender<DocumentEvent>,
    data: T,
}

impl<T: Send + Sync + 'static> ResourceHandler<T> {
    pub(crate) fn new(
        tx: Sender<DocumentEvent>,
        doc_id: usize,
        node_id: Option<usize>,
        data: T,
    ) -> Self {
        static REQUEST_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
        Self {
            request_id: REQUEST_ID_COUNTER.fetch_add(1, Ao::Relaxed),
            doc_id,
            node_id,
            tx,
            data,
        }
    }

    pub(crate) fn boxed(
        tx: Sender<DocumentEvent>,
        doc_id: usize,
        node_id: Option<usize>,
        data: T,
    ) -> Box<dyn NetHandler>
    where
        ResourceHandler<T>: NetHandler,
    {
        Box::new(Self::new(tx, doc_id, node_id, data)) as _
    }

    // fn handle(self: Box<Self>, resolved_url: String, bytes: Bytes) -> ResourceLoadResponse {
    //     ResourceLoadResponse {
    //         request_id: self.request_id,
    //         node_id: self.node_id,
    //         resolved_url: Some(resolved_url),
    //         result: self.data.parse(bytes, self.doc_id),
    //     }
    // }

    fn respond(self: Box<Self>, resolved_url: String, result: Result<Resource, String>) {
        let response = ResourceLoadResponse {
            request_id: self.request_id,
            node_id: self.node_id,
            resolved_url: Some(resolved_url),
            result,
        };
        let _ = self.tx.send(DocumentEvent::ResourceLoad(response));
    }
}

// impl<T: ParseResource> NetHandler for ResourceHandler<T> {
//     fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
//         self.tx.send(DocumentEvent::ResourceLoad(self.handle(resolved_url, bytes)));
//     }
// }

#[allow(unused)]
pub struct ResourceLoadResponse {
    pub request_id: usize,
    pub node_id: Option<usize>,
    pub resolved_url: Option<String>,
    pub result: Result<Resource, String>,
}

pub struct StylesheetHandler {
    pub source_url: Url,
    pub guard: SharedRwLock,
    pub net_provider: SharedProvider<Resource>,
}

impl NetHandler for ResourceHandler<StylesheetHandler> {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let Ok(css) = std::str::from_utf8(&bytes) else {
            return self.respond(resolved_url, Err(String::from("Invalid UTF8")));
        };

        // NOTE(Nico): I don't *think* external stylesheets should have HTML entities escaped
        // let escaped_css = html_escape::decode_html_entities(css);

        let sheet = Stylesheet::from_str(
            css,
            self.data.source_url.clone().into(),
            Origin::Author,
            ServoArc::new(self.data.guard.wrap(MediaList::empty())),
            self.data.guard.clone(),
            Some(&StylesheetLoader {
                tx: self.tx.clone(),
                doc_id: self.doc_id,
                net_provider: self.data.net_provider.clone(),
            }),
            None, // error_reporter
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );

        // Fetch @font-face fonts
        fetch_font_face(
            self.tx.clone(),
            self.doc_id,
            self.node_id.unwrap(),
            &sheet,
            &self.data.net_provider,
            &self.data.guard.read(),
        );

        self.respond(
            resolved_url,
            Ok(Resource::Css(DocumentStyleSheet(ServoArc::new(sheet)))),
        );
    }
}

#[derive(Clone)]
pub(crate) struct StylesheetLoader {
    pub(crate) tx: Sender<DocumentEvent>,
    pub(crate) doc_id: usize,
    pub(crate) net_provider: SharedProvider<Resource>,
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
        if !supports.as_ref().is_none_or(|s| s.enabled) {
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
            Request::get(url.as_ref().clone()),
            ResourceHandler::boxed(
                self.tx.clone(),
                self.doc_id,
                None, // node_id
                NestedStylesheetHandler {
                    url: url.clone(),
                    loader: self.clone(),
                    lock: lock.clone(),
                    media,
                    import_rule: import.clone(),
                    provider: self.net_provider.clone(),
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
    provider: SharedProvider<Resource>,
}

impl NetHandler for ResourceHandler<NestedStylesheetHandler> {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let Ok(css) = std::str::from_utf8(&bytes) else {
            return self.respond(resolved_url, Err(String::from("Invalid UTF8")));
        };

        // NOTE(Nico): I don't *think* external stylesheets should have HTML entities escaped
        // let escaped_css = html_escape::decode_html_entities(css);

        let sheet = ServoArc::new(Stylesheet::from_str(
            css,
            UrlExtraData(self.data.url.clone()),
            Origin::Author,
            self.data.media.clone(),
            self.data.lock.clone(),
            Some(&self.data.loader),
            None, // error_reporter
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        ));

        // Fetch @font-face fonts
        fetch_font_face(
            self.tx.clone(),
            self.doc_id,
            self.node_id.unwrap(),
            &sheet,
            &self.data.provider,
            &self.data.lock.read(),
        );

        let mut guard = self.data.lock.write();
        self.data.import_rule.write_with(&mut guard).stylesheet = ImportSheet::Sheet(sheet);
        drop(guard);

        self.respond(resolved_url, Ok(Resource::None))
    }
}

struct FontFaceHandler(FontFaceSourceFormatKeyword);
impl NetHandler for ResourceHandler<FontFaceHandler> {
    fn bytes(mut self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let result = self.data.parse(bytes);
        self.respond(resolved_url, result)
    }
}
impl FontFaceHandler {
    fn parse(&mut self, bytes: Bytes) -> Result<Resource, String> {
        if self.0 == FontFaceSourceFormatKeyword::None && bytes.len() >= 4 {
            self.0 = match &bytes.as_ref()[0..4] {
                // WOFF (v1) files begin with 0x774F4646 ('wOFF' in ascii)
                // See: <https://w3c.github.io/woff/woff1/spec/Overview.html#WOFFHeader>
                #[cfg(any(feature = "woff-c", feature = "woff-rust"))]
                b"wOFF" => FontFaceSourceFormatKeyword::Woff,
                // WOFF2 files begin with 0x774F4632 ('wOF2' in ascii)
                // See: <https://w3c.github.io/woff/woff2/#woff20Header>
                #[cfg(any(feature = "woff-c", feature = "woff-rust"))]
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
        #[cfg(any(feature = "woff-c", feature = "woff-rust"))]
        let mut bytes = bytes;

        match self.0 {
            #[cfg(any(feature = "woff-c", feature = "woff-rust"))]
            FontFaceSourceFormatKeyword::Woff => {
                #[cfg(feature = "tracing")]
                tracing::info!("Decompressing woff1 font");

                // Use woff crate to decompress font
                #[cfg(feature = "woff-c")]
                let decompressed = woff::version1::decompress(&bytes);

                // Use wuff crate to decompress font
                #[cfg(feature = "woff-rust")]
                let decompressed = wuff::decompress_woff1(&bytes).ok();

                if let Some(decompressed) = decompressed {
                    bytes = Bytes::from(decompressed);
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("Failed to decompress woff1 font");
                }
            }
            #[cfg(any(feature = "woff-c", feature = "woff-rust"))]
            FontFaceSourceFormatKeyword::Woff2 => {
                #[cfg(feature = "tracing")]
                tracing::info!("Decompressing woff2 font");

                // Use woff crate to decompress font
                #[cfg(feature = "woff-c")]
                let decompressed = woff::version2::decompress(&bytes);

                // Use wuff crate to decompress font
                #[cfg(feature = "woff-rust")]
                let decompressed = wuff::decompress_woff2(&bytes).ok();

                if let Some(decompressed) = decompressed {
                    bytes = Bytes::from(decompressed);
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("Failed to decompress woff2 font");
                }
            }
            FontFaceSourceFormatKeyword::None => {
                // Should this be an error?
                return Ok(Resource::None);
            }
            _ => {}
        }

        return Ok(Resource::Font(bytes));
    }
}

fn fetch_font_face(
    tx: Sender<DocumentEvent>,
    doc_id: usize,
    node_id: usize,
    sheet: &Stylesheet,
    network_provider: &SharedProvider<Resource>,
    read_guard: &SharedRwLockReadGuard,
) {
    sheet
        .contents(read_guard)
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
            network_provider.fetch(
                doc_id,
                Request::get(url),
                ResourceHandler::boxed(tx.clone(), doc_id, Some(node_id), FontFaceHandler(format)),
            );
        });
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
        // Try parse image
        if let Ok(image) = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .expect("IO errors impossible with Cursor")
            .decode()
        {
            let raw_rgba8_data = image.clone().into_rgba8().into_raw();
            return Ok(Resource::Image(
                self.kind,
                image.width(),
                image.height(),
                Arc::new(raw_rgba8_data),
            ));
        };

        #[cfg(feature = "svg")]
        {
            use crate::util::parse_svg;
            if let Ok(tree) = parse_svg(&bytes) {
                return Ok(Resource::Svg(self.kind, Box::new(tree)));
            }
        }

        return Err(String::from("Could not parse image"));
    }
}
