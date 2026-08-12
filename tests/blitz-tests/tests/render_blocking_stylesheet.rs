//! Regression test for https://github.com/DioxusLabs/blitz/issues/689
//!
//! Style resolution must not run while render-blocking stylesheets are still
//! loading. If it does, elements get computed styles from an incomplete
//! cascade, and the restyle triggered by the stylesheet's arrival spuriously
//! starts CSS transitions from those unstyled values (e.g. `visibility:
//! visible` -> `hidden`), leaving transitioned properties stuck at their
//! pre-stylesheet values in one-shot renders.

use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use std::sync::{Arc, Mutex};
use style::properties::generated::longhands::visibility::computed_value::T as Visibility;

/// A `NetProvider` which records requests so the test can deliver
/// responses at a time of its choosing.
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

const HTML: &str = r#"<!doctype html>
<html>
  <head><link rel="stylesheet" href="case.css"></head>
  <body><p id="transitioned">text</p></body>
</html>"#;

const CSS: &str = "#transitioned { transition: all 0.2s; visibility: hidden }";

#[test]
fn transition_does_not_start_from_pre_stylesheet_styles() {
    let net = Arc::new(ManualNetProvider::default());

    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            base_url: Some("http://example.com/".to_string()),
            net_provider: Some(Arc::clone(&net) as _),
            ..Default::default()
        },
    );

    // The stylesheet fetch should have been issued and be tracked as a
    // pending critical resource.
    assert!(doc.has_pending_critical_resources());

    // Resolve while the stylesheet is still loading. This must not give
    // elements computed styles (which would later be treated as
    // before-change styles and start spurious transitions).
    doc.resolve(0.0);

    // Deliver the stylesheet and resolve again.
    let (url, handler) = net.requests.lock().unwrap().pop().expect("css requested");
    assert!(url.ends_with("case.css"));
    handler.bytes(url, Bytes::from_static(CSS.as_bytes()));
    doc.resolve(1.0);
    assert!(!doc.has_pending_critical_resources());

    // The element must be hidden immediately: no transition from the
    // pre-stylesheet `visibility: visible` should have started.
    let node_id = doc.get_element_by_id("transitioned").unwrap();
    let node = doc.get_node(node_id).unwrap();
    let styles = node.primary_styles().unwrap();
    assert_eq!(
        styles.get_inherited_box().clone_visibility(),
        Visibility::Hidden,
        "transitioned property should have its stylesheet value, not the pre-stylesheet value"
    );

    // And nothing should be animating.
    assert!(!doc.is_animating());
}
