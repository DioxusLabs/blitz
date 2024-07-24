use std::io::{Cursor, Read};

use crate::node::{Node, NodeData};
use image::DynamicImage;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";
const FILE_SIZE_LIMIT: u64 = 1_000_000_000; // 1GB

pub(crate) fn fetch_blob(url: &str) -> Result<Vec<u8>, Box<ureq::Error>> {
    if url.starts_with("data:") {
        let data_url = data_url::DataUrl::process(url).unwrap();
        let decoded = data_url.decode_to_vec().expect("Invalid data url");
        return Ok(decoded.0);
    }

    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(Box::new)?;

    let len: usize = resp
        .header("Content-Length")
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let mut bytes: Vec<u8> = Vec::with_capacity(len);

    resp.into_reader()
        .take(FILE_SIZE_LIMIT)
        .read_to_end(&mut bytes)
        .unwrap();

    Ok(bytes)
}

pub(crate) fn fetch_string(url: &str) -> Result<String, Box<ureq::Error>> {
    fetch_blob(url).map(|vec| String::from_utf8(vec).expect("Invalid UTF8"))
}

// pub(crate) fn fetch_buffered_stream(
//     url: &str,
// ) -> Result<impl BufRead + Read + Send + Sync, ureq::Error> {
//     let resp = ureq::get(url).set("User-Agent", USER_AGENT).call()?;
//     Ok(BufReader::new(resp.into_reader().take(FILE_SIZE_LIMIT)))
// }

#[allow(unused)]
pub(crate) enum ImageFetchErr {
    FetchErr(Box<ureq::Error>),
    ImageError(image::error::ImageError),
}
impl From<Box<ureq::Error>> for ImageFetchErr {
    fn from(value: Box<ureq::Error>) -> Self {
        Self::FetchErr(value)
    }
}
impl From<image::error::ImageError> for ImageFetchErr {
    fn from(value: image::error::ImageError) -> Self {
        Self::ImageError(value)
    }
}

pub(crate) fn fetch_image(url: &str) -> Result<DynamicImage, ImageFetchErr> {
    let blob = crate::util::fetch_blob(url)?;
    let image = image::ImageReader::new(Cursor::new(blob))
        .with_guessed_format()
        .expect("IO errors impossible with Cursor")
        .decode()?;
    Ok(image)
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
use style::color::AbsoluteColor;

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
