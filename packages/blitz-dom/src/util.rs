use crate::node::{Node, NodeData};
use image::DynamicImage;
use selectors::context::QuirksMode;
use std::str::FromStr;
use std::{
    io::Cursor,
    sync::{Arc, OnceLock},
};
use style::{
    color::AbsoluteColor,
    media_queries::MediaList,
    servo_arc::Arc as ServoArc,
    shared_lock::SharedRwLock,
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
};
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
    pub provider: SharedProvider<Resource>,
}
impl RequestHandler for CssHandler {
    type Data = Resource;
    fn bytes(self: Box<Self>, bytes: Bytes, callback: SharedCallback<Self::Data>) {
        let css = std::str::from_utf8(&bytes).expect("Invalid UTF8");
        let escaped_css = html_escape::decode_html_entities(css);
        let sheet = Stylesheet::from_str(
            &escaped_css,
            self.source_url.into(),
            Origin::Author,
            ServoArc::new(self.guard.wrap(MediaList::empty())),
            self.guard.clone(),
            None,
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );
        let read_guard = self.guard.read();

        sheet
            .rules(&read_guard)
            .iter()
            .filter_map(|rule| match rule {
                CssRule::FontFace(font_face) => font_face.read_with(&read_guard).sources.as_ref(),
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
                self.provider.fetch(
                    Url::from_str(url_source.url.as_str()).unwrap(),
                    Box::new(FontFaceHandler(format)),
                )
            });

        callback.call(Resource::Css(
            self.node,
            DocumentStyleSheet(ServoArc::new(sheet)),
        ))
    }
}
struct FontFaceHandler(FontFaceSourceFormatKeyword);
impl RequestHandler for FontFaceHandler {
    type Data = Resource;
    fn bytes(mut self: Box<Self>, mut bytes: Bytes, callback: SharedCallback<Self::Data>) {
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

        callback.call(Resource::Font(bytes))
    }
}
pub(crate) struct ImageHandler(usize);
impl ImageHandler {
    pub(crate) fn new(node_id: usize) -> Self {
        Self(node_id)
    }
}
impl RequestHandler for ImageHandler {
    type Data = Resource;
    fn bytes(self: Box<Self>, bytes: Bytes, callback: SharedCallback<Self::Data>) {
        // Try parse image
        if let Ok(image) = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .expect("IO errors impossible with Cursor")
            .decode()
        {
            callback.call(Resource::Image(self.0, Arc::new(image)));
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
        callback.call(Resource::Svg(self.0, Box::new(tree)));
    }
}

// Debug print an RcDom
pub fn walk_tree(indent: usize, node: &Node) {
    // Skip all-whitespace text nodes entirely
    if let NodeData::Text(data) = &node.raw_dom_data {
        if data.content.chars().all(|c| c.is_ascii_whitespace()) {
            return;
        }
    }

    print!("{}", " ".repeat(indent));
    let id = node.id;
    match &node.raw_dom_data {
        NodeData::Document => println!("#Document {id}"),

        NodeData::Text(data) => {
            if data.content.chars().all(|c| c.is_ascii_whitespace()) {
                println!("{id} #text: <whitespace>");
            } else {
                let content = data.content.trim();
                if content.len() > 10 {
                    println!(
                        "#text {id}: {}...",
                        content
                            .split_at(content.char_indices().take(10).last().unwrap().0)
                            .0
                            .escape_default()
                    )
                } else {
                    println!("#text {id}: {}", data.content.trim().escape_default())
                }
            }
        }

        NodeData::Comment => println!("<!-- COMMENT {id} -->"),

        NodeData::AnonymousBlock(_) => println!("{id} AnonymousBlock"),

        NodeData::Element(data) => {
            print!("<{} {id}", data.name.local);
            for attr in data.attrs.iter() {
                print!(" {}=\"{}\"", attr.name.local, attr.value);
            }
            if !node.children.is_empty() {
                println!(">");
            } else {
                println!("/>");
            }
        } // NodeData::Doctype {
          //     ref name,
          //     ref public_id,
          //     ref system_id,
          // } => println!("<!DOCTYPE {} \"{}\" \"{}\">", name, public_id, system_id),
          // NodeData::ProcessingInstruction { .. } => unreachable!(),
    }

    if !node.children.is_empty() {
        for child_id in node.children.iter() {
            walk_tree(indent + 2, node.with(*child_id));
        }

        if let NodeData::Element(data) = &node.raw_dom_data {
            println!("{}</{}>", " ".repeat(indent), data.name.local);
        }
    }
}

use blitz_traits::net::{Bytes, RequestHandler, SharedCallback, SharedProvider};
use peniko::Color as PenikoColor;
use style::font_face::{FontFaceSourceFormat, FontFaceSourceFormatKeyword, Source};
use style::stylesheets::{CssRule, StylesheetInDocument};

pub trait ToPenikoColor {
    fn as_peniko(&self) -> PenikoColor;
}
impl ToPenikoColor for AbsoluteColor {
    fn as_peniko(&self) -> PenikoColor {
        let [r, g, b, a] = self
            .to_color_space(style::color::ColorSpace::Srgb)
            .raw_components()
            .map(|f| (f * 255.0) as u8);
        PenikoColor { r, g, b, a }
    }
}
