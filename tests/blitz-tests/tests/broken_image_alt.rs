//! An `<img>` whose image can't be obtained (its URL can't be resolved, or the
//! fetch fails) renders like a browser's broken image: its alt text is laid
//! out in the image's box, rather than the image rendering as nothing.
//! https://html.spec.whatwg.org/multipage/rendering.html#images-3

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::net::{NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::{Arc, Mutex};

/// A `NetProvider` which records requests so the test can fail them.
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

fn config(base_url: Option<&str>, net: Option<Arc<ManualNetProvider>>) -> DocumentConfig {
    DocumentConfig {
        base_url: base_url.map(str::to_string),
        viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
        net_provider: net.map(|net| net as _),
        ..Default::default()
    }
}

/// The laid-out size of `#img` and the text of its inline layout, if any
fn img_box(doc: &HtmlDocument) -> ((f32, f32), Option<String>) {
    let img_id = doc.query_selector("#img").unwrap().expect("#img not found");
    let node = doc.get_node(img_id).unwrap();
    let layout = node.final_layout();
    let text = node
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .map(|layout| layout.text.clone());
    ((layout.size.width, layout.size.height), text)
}

#[test]
fn unresolvable_src_renders_alt_text() {
    // Without a base url, the relative src can't be resolved
    let mut doc = HtmlDocument::from_html(
        r#"<body style="margin:0"><img id="img" src="missing.png" alt="Broken image"></body>"#,
        config(None, None),
    );
    doc.resolve(0.0);

    let ((width, height), text) = img_box(&doc);
    assert_eq!(text.as_deref(), Some("Broken image"));
    assert!(width > 0.0 && height > 0.0, "box is {width}x{height}");
}

#[test]
fn failed_fetch_renders_alt_text_in_sized_box() {
    let net = Arc::new(ManualNetProvider::default());
    let mut doc = HtmlDocument::from_html(
        r#"<body style="margin:0"><img id="img" src="missing.png" alt="Broken image" width="200" height="100"></body>"#,
        config(Some("http://example.com/"), Some(net.clone())),
    );
    doc.resolve(0.0);
    assert_eq!(img_box(&doc), ((200.0, 100.0), None));

    let (url, handler) = net.requests.lock().unwrap().pop().unwrap();
    assert_eq!(url, "http://example.com/missing.png");
    handler.error(url, "404 Not Found".to_string());
    doc.resolve(0.0);

    assert_eq!(
        img_box(&doc),
        ((200.0, 100.0), Some("Broken image".to_string()))
    );
}

#[test]
fn broken_image_with_empty_alt_renders_nothing() {
    let mut doc = HtmlDocument::from_html(
        r#"<body style="margin:0"><img id="img" src="missing.png" alt=""></body>"#,
        config(None, None),
    );
    doc.resolve(0.0);

    assert_eq!(img_box(&doc), ((0.0, 0.0), None));
}
