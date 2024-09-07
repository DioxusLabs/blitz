use crate::node::{Node, NodeData};
use blitz_net::RequestHandler;
use image::DynamicImage;
use selectors::context::QuirksMode;
use std::any::{Any, TypeId};
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
    Image(usize, DynamicImage),
    Svg(usize, Tree),
    Css(usize, DocumentStyleSheet),
}
pub(crate) struct CssMarker;
pub(crate) struct CssHandler {
    pub node: usize,
    pub source_url: Url,
    pub guard: SharedRwLock,
}
impl RequestHandler<Resource> for CssHandler {
    fn bytes(self, bytes: &[u8]) -> Resource {
        let css = String::from_utf8(bytes.into()).expect("Invalid UTF8");
        let escaped_css = html_escape::decode_html_entities(&css);
        let sheet = Stylesheet::from_str(
            &escaped_css,
            self.source_url.into(),
            Origin::Author,
            ServoArc::new(self.guard.wrap(MediaList::empty())),
            self.guard,
            None,
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );
        Resource::Css(self.node, DocumentStyleSheet(ServoArc::new(sheet)))
    }
    fn special(&self) -> Option<TypeId> {
        Some(CssMarker.type_id())
    }
}
pub(crate) struct ImageHandler(usize);
impl ImageHandler {
    pub(crate) fn new(node_id: usize) -> Self {
        Self(node_id)
    }
}
impl RequestHandler<Resource> for ImageHandler {
    fn bytes(self, bytes: &[u8]) -> Resource {
        // Try parse image
        if let Ok(image) = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .expect("IO errors impossible with Cursor")
            .decode()
        {
            return Resource::Image(self.0, image);
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

        let tree = Tree::from_data(bytes, &options).unwrap();
        Resource::Svg(self.0, tree)
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

use peniko::Color as PenikoColor;

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
