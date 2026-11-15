use blitz_test_harness::{Harness, HarnessOptions};
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use std::sync::{Arc, Mutex};

struct StylesheetNet {
    responses: Vec<(&'static str, &'static str, Option<&'static [u8]>)>,
    requests: Mutex<Vec<String>>,
}

impl NetProvider for StylesheetNet {
    fn fetch(&self, _: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.to_string();
        self.requests.lock().unwrap().push(url.clone());
        match self
            .responses
            .iter()
            .find(|(path, _, _)| *path == request.url.path())
        {
            Some((_, resolved, Some(bytes))) => {
                handler.bytes((*resolved).into(), Bytes::from_static(bytes))
            }
            _ => handler.error(url, "not found".into()),
        }
    }
}

fn document(net: Arc<StylesheetNet>) -> Harness {
    Harness::from_html_with(
        "<link rel=stylesheet href='old/entry.css'><div id=box></div>",
        HarnessOptions {
            base_url: Some("https://example.test/".into()),
            net_provider: Some(net),
            ..Default::default()
        },
    )
}

#[test]
fn redirected_stylesheets_use_each_response_url_for_relative_resources() {
    let net = Arc::new(StylesheetNet {
        responses: vec![
            (
                "/old/entry.css",
                "https://example.test/assets/css/entry.css",
                Some(b"@import 'nested/child.css';"),
            ),
            (
                "/assets/css/nested/child.css",
                "https://example.test/final/child.css",
                Some(b"@import 'grand.css'; #box {background-image:url('../image.svg')}"),
            ),
            (
                "/final/grand.css",
                "https://example.test/final/grand.css",
                Some(b"#box {width:23px;height:17px}"),
            ),
        ],
        requests: Mutex::new(Vec::new()),
    });
    let harness = document(net.clone());
    let requests = net.requests.lock().unwrap();
    for url in [
        "https://example.test/assets/css/nested/child.css",
        "https://example.test/final/grand.css",
        "https://example.test/image.svg",
    ] {
        assert!(
            requests.iter().any(|request| request == url),
            "{url}: {requests:?}"
        );
    }
    assert_eq!(harness.layout_rect("#box").width, 23.0);
    assert_eq!(harness.layout_rect("#box").height, 17.0);
    assert!(!harness.base().has_pending_critical_resources());
}

#[test]
fn unchanged_response_url_keeps_relative_imports() {
    let net = Arc::new(StylesheetNet {
        responses: vec![
            (
                "/old/entry.css",
                "https://example.test/old/entry.css",
                Some(b"@import 'child.css';"),
            ),
            (
                "/old/child.css",
                "https://example.test/old/child.css",
                Some(b"#box {width:23px}"),
            ),
        ],
        requests: Mutex::new(Vec::new()),
    });
    assert_eq!(document(net).layout_rect("#box").width, 23.0);
}

#[test]
fn imported_stylesheet_uses_its_own_response_url() {
    let net = Arc::new(StylesheetNet {
        responses: vec![
            (
                "/old/entry.css",
                "https://example.test/old/entry.css",
                Some(b"@import 'child.css';"),
            ),
            (
                "/old/child.css",
                "https://example.test/final/child.css",
                Some(b"@import 'grand.css';"),
            ),
            (
                "/final/grand.css",
                "https://example.test/final/grand.css",
                Some(b"#box {width:23px}"),
            ),
        ],
        requests: Mutex::new(Vec::new()),
    });
    assert_eq!(document(net).layout_rect("#box").width, 23.0);
}

#[test]
fn invalid_response_url_keeps_the_request_base() {
    let net = Arc::new(StylesheetNet {
        responses: vec![
            ("/old/entry.css", "not a URL", Some(b"@import 'child.css';")),
            ("/old/child.css", "not a URL", Some(b"@import 'grand.css';")),
            (
                "/old/grand.css",
                "https://example.test/old/grand.css",
                Some(b"#box {width:23px}"),
            ),
        ],
        requests: Mutex::new(Vec::new()),
    });
    assert_eq!(document(net).layout_rect("#box").width, 23.0);
}

#[test]
fn empty_or_failed_response_unblocks_rendering() {
    for body in [Some(b"".as_slice()), None] {
        let net = Arc::new(StylesheetNet {
            responses: vec![(
                "/old/entry.css",
                "https://example.test/assets/entry.css",
                body,
            )],
            requests: Mutex::new(Vec::new()),
        });
        let harness = document(net.clone());
        assert!(!harness.base().has_pending_critical_resources());
        assert_eq!(net.requests.lock().unwrap().len(), 1);
    }
}
