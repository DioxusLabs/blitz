use std::io::Read;

use markup5ever_rcdom::{Handle, NodeData};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";
const FILE_SIZE_LIMIT: u64 = 1_000_000_000; // 1GB

pub(crate) fn fetch_string(url: &str) -> Result<String, ureq::Error> {
    Ok(ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()?
        .into_string()?)
}

pub(crate) fn fetch_blob(url: &str) -> Result<Vec<u8>, ureq::Error> {
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

// Debug print an RcDom
pub (crate) fn walk_rc_dom(indent: usize, handle: &Handle) {
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