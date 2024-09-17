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
                self.provider.fetch(
                    Url::from_str(url_source.url.as_str()).unwrap(),
                    Box::new(FontFaceHandler),
                )
            });

        callback.call(Resource::Css(
            self.node,
            DocumentStyleSheet(ServoArc::new(sheet)),
        ))
    }
}
struct FontFaceHandler;
impl RequestHandler for FontFaceHandler {
    type Data = Resource;
    fn bytes(self: Box<Self>, bytes: Bytes, callback: SharedCallback<Self::Data>) {
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

        let tree = Tree::from_data(&bytes, &options).unwrap();
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
    match &node.raw_dom_data {
        NodeData::Document => println!("#Document"),

        NodeData::Text(data) => {
            if data.content.chars().all(|c| c.is_ascii_whitespace()) {
                println!("#text: <whitespace>");
            } else {
                let content = data.content.trim();
                if content.len() > 10 {
                    println!(
                        "#text: {}...",
                        content
                            .split_at(content.char_indices().take(10).last().unwrap().0)
                            .0
                            .escape_default()
                    )
                } else {
                    println!("#text: {}", data.content.trim().escape_default())
                }
            }
        }

        NodeData::Comment => println!("<!-- COMMENT -->"),

        NodeData::AnonymousBlock(_) => println!("AnonymousBlock"),

        NodeData::Element(data) => {
            print!("<{}", data.name.local);
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
use style::font_face::Source;
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
