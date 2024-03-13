use std::io::{BufRead, BufReader, Cursor, Read};

use image::DynamicImage;
use markup5ever_rcdom::{Handle, NodeData};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";
const FILE_SIZE_LIMIT: u64 = 1_000_000_000; // 1GB

pub(crate) fn fetch_blob(url: &str) -> Result<Vec<u8>, ureq::Error> {
    if url.starts_with("data:") {
        let data_url = data_url::DataUrl::process(url).unwrap();
        let decoded = data_url.decode_to_vec().expect("Invalid data url");
        return Ok(decoded.0);
    }

    let resp = ureq::get(url).set("User-Agent", USER_AGENT).call()?;

    let len: usize = resp
        .header("Content-Length")
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let mut bytes: Vec<u8> = Vec::with_capacity(len);

    resp.into_reader()
        .take(FILE_SIZE_LIMIT)
        .read_to_end(&mut bytes)?;

    Ok(bytes)
}

pub(crate) fn fetch_string(url: &str) -> Result<String, ureq::Error> {
    fetch_blob(url).map(|vec| String::from_utf8(vec).expect("Invalid UTF8"))
}

pub(crate) fn fetch_buffered_stream(
    url: &str,
) -> Result<impl BufRead + Read + Send + Sync, ureq::Error> {
    let resp = ureq::get(url).set("User-Agent", USER_AGENT).call()?;
    Ok(BufReader::new(resp.into_reader().take(FILE_SIZE_LIMIT)))
}

pub(crate) enum ImageFetchErr {
    FetchErr(ureq::Error),
    ImageError(image::error::ImageError),
}
impl From<ureq::Error> for ImageFetchErr {
    fn from(value: ureq::Error) -> Self {
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
    let image = image::io::Reader::new(Cursor::new(blob))
        .with_guessed_format()
        .expect("IO errors impossible with Cursor")
        .decode()?;
    Ok(image)
}

// Debug print an RcDom
pub(crate) fn walk_rc_dom(indent: usize, handle: &Handle) {
    let node = handle;
    for _ in 0..indent {
        print!(" ");
    }
    match node.data {
        NodeData::Document => println!("#Document"),

        NodeData::Doctype {
            ref name,
            ref public_id,
            ref system_id,
        } => println!("<!DOCTYPE {} \"{}\" \"{}\">", name, public_id, system_id),

        NodeData::Text { ref contents } => {
            println!("#text: {}", contents.borrow().escape_default())
        }

        NodeData::Comment { ref contents } => println!("<!-- {} -->", contents.escape_default()),

        NodeData::Element {
            ref name,
            ref attrs,
            ..
        } => {
            // assert!(name.ns == ns!(html));
            print!("<{}", name.local);
            for attr in attrs.borrow().iter() {
                // assert!(attr.name.ns == ns!());
                print!(" {}=\"{}\"", attr.name.local, attr.value);
            }
            println!(">");
        }

        NodeData::ProcessingInstruction { .. } => unreachable!(),
    }

    for child in node.children.borrow().iter() {
        walk_rc_dom(indent + 4, child);
    }
}
