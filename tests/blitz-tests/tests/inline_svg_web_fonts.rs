//! Inline `<svg>` text is laid out by usvg, which sees only its own font database. It must still
//! find the page's `@font-face` fonts under their CSS family names, as SVG text does in browsers.

use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use std::sync::{Arc, Mutex};

/// A `NetProvider` which records requests so the test can answer them.
#[derive(Default)]
struct ManualNetProvider {
    requests: Mutex<Vec<(String, Box<dyn NetHandler>)>>,
}

impl NetProvider for ManualNetProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        self.requests
            .lock()
            .unwrap()
            .push((request.url.to_string(), handler));
    }
}

/// Load a document whose `@font-face` rule (with `descriptors`) serves the bullet font, and
/// return the families of the faces usvg drew the SVG's bullet with.
fn svg_text_families(descriptors: &str) -> Vec<String> {
    let html = format!(
        r#"<html><head><style>@font-face {{ font-family: Web; src: url(bullet.otf); {descriptors} }}</style></head>
        <body><svg id="icon" width="100" height="40"><text x="0" y="30" font-family="Web" font-size="30">•</text></svg></body></html>"#
    );
    let net = Arc::new(ManualNetProvider::default());
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some("http://example.com/".to_string()),
            net_provider: Some(net.clone()),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let requests = std::mem::take(&mut *net.requests.lock().unwrap());
    for (url, handler) in requests {
        assert!(url.ends_with("bullet.otf"), "unexpected request {url}");
        handler.bytes(url, Bytes::from_static(blitz_dom::BULLET_FONT));
    }
    doc.resolve(0.0);

    let svg_id = doc.query_selector("#icon").unwrap().unwrap();
    let tree = doc
        .get_node(svg_id)
        .unwrap()
        .element_data()
        .unwrap()
        .svg_data()
        .expect("inline svg parsed");
    let mut families = Vec::new();
    collect_families(tree.root(), tree.fontdb(), &mut families);
    families
}

fn collect_families(group: &usvg::Group, db: &usvg::fontdb::Database, out: &mut Vec<String>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => collect_families(group, db, out),
            usvg::Node::Text(text) => {
                for span in text.layouted() {
                    for glyph in &span.positioned_glyphs {
                        if let Some(face) = db.face(glyph.font) {
                            out.extend(face.families.iter().map(|(name, _)| name.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[test]
fn inline_svg_text_uses_font_face_fonts() {
    let families = svg_text_families("");
    assert!(
        !families.is_empty() && families.iter().all(|family| family == "Web"),
        "{families:?}"
    );
}

#[test]
fn inline_svg_text_skips_font_face_subsets_without_latin() {
    // usvg picks one face per family for a whole span, so a subset for another script would be
    // chosen for Latin text and fall back per character; such faces are not registered.
    let families = svg_text_families("unicode-range: U+0400-045F;");
    assert!(
        !families.iter().any(|family| family == "Web"),
        "{families:?}"
    );
}
