//! `@font-face` rules nested in `@layer` and matching `@media` and `@supports`
//! rules are loaded, as browsers do (https://github.com/DioxusLabs/blitz/issues/1506).

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

impl ManualNetProvider {
    fn take_requests(&self) -> Vec<(String, Box<dyn NetHandler>)> {
        std::mem::take(&mut *self.requests.lock().unwrap())
    }
}

const NESTED_RULES: &str = "
    @font-face { font-family: top; src: url(top.ttf); }
    @layer base { @font-face { font-family: layer; src: url(layer.ttf); } }
    @media screen { @font-face { font-family: media; src: url(media.ttf); } }
    @media print { @font-face { font-family: print; src: url(print.ttf); } }
    @supports (display: block) { @font-face { font-family: supports; src: url(supports.ttf); } }
    @supports not (display: block) { @font-face { font-family: unsupported; src: url(unsupported.ttf); } }
    @layer outer { @media all { @supports (color: red) {
        @font-face { font-family: deep; src: url(deep.ttf); }
    } } }
";

const EXPECTED_FONTS: [&str; 5] = [
    "http://example.com/deep.ttf",
    "http://example.com/layer.ttf",
    "http://example.com/media.ttf",
    "http://example.com/supports.ttf",
    "http://example.com/top.ttf",
];

fn load(html: &str) -> (HtmlDocument, Arc<ManualNetProvider>) {
    let net = Arc::new(ManualNetProvider::default());
    let doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            base_url: Some("http://example.com/".to_string()),
            net_provider: Some(net.clone()),
            ..Default::default()
        },
    );
    (doc, net)
}

fn sorted_urls(requests: &[(String, Box<dyn NetHandler>)]) -> Vec<&str> {
    let mut urls: Vec<&str> = requests.iter().map(|(url, _)| url.as_str()).collect();
    urls.sort();
    urls
}

#[test]
fn nested_font_faces_in_style_element_are_fetched() {
    let (_doc, net) = load(&format!(
        "<html><head><style>{NESTED_RULES}</style></head><body></body></html>"
    ));
    assert_eq!(sorted_urls(&net.take_requests()), EXPECTED_FONTS);
}

#[test]
fn nested_font_faces_in_imported_stylesheet_are_fetched() {
    let (mut doc, net) =
        load("<html><head><style>@import url(imported.css);</style></head><body></body></html>");
    let requests = net.take_requests();
    assert_eq!(sorted_urls(&requests), ["http://example.com/imported.css"]);
    for (url, handler) in requests {
        handler.bytes(url, Bytes::from_static(NESTED_RULES.as_bytes()));
    }
    doc.resolve(0.0);
    assert_eq!(sorted_urls(&net.take_requests()), EXPECTED_FONTS);
}
