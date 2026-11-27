use std::sync::{Arc, Mutex};

use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::net::{NetHandler, NetProvider, Request, http::header::ACCEPT};

#[derive(Default)]
struct RecordingNetwork(Mutex<Vec<Request>>);

impl NetProvider for RecordingNetwork {
    fn fetch(&self, _: usize, request: Request, _: Box<dyn NetHandler>) {
        self.0.lock().unwrap().push(request);
    }
}

#[test]
fn image_requests_accept_image_responses() {
    let network = Arc::new(RecordingNetwork::default());
    let _document = HtmlDocument::from_html(
        r#"<img src="https://example.test/icon">"#,
        DocumentConfig {
            net_provider: Some(network.clone()),
            ..Default::default()
        },
    );
    let requests = network.0.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].headers[ACCEPT], "image/*, */*;q=0.8");
}
