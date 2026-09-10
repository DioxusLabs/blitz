//! A `<link>` starts loading its stylesheet once it has both `rel=stylesheet`
//! and an `href`, whichever of the two a script sets last.
//!
//! The insertion hook runs before a script has set any attribute, so the load
//! has to be re-triggered by the attribute that completes the pair; only
//! `href` did, and a link given its `href` first and its `rel` second was never
//! fetched.

use blitz_dom::{DocumentConfig, QualName, local_name, ns};
use blitz_html::HtmlDocument;
use blitz_traits::net::{NetHandler, NetProvider, Request};
use std::sync::{Arc, Mutex};

/// A `NetProvider` which records the urls it is asked for.
#[derive(Default)]
struct RecordingNetProvider {
    requests: Mutex<Vec<String>>,
}

impl NetProvider for RecordingNetProvider {
    fn fetch(&self, _doc_id: usize, request: Request, _handler: Box<dyn NetHandler>) {
        self.requests.lock().unwrap().push(request.url.to_string());
    }
}

fn attr(name: &str) -> QualName {
    QualName::new(None, ns!(), name.into())
}

/// An empty document with a recording net provider and a `<link>` freshly
/// appended to its head, carrying no attributes yet.
fn doc_with_link() -> (HtmlDocument, blitz_dom::NodeId, Arc<RecordingNetProvider>) {
    let net = Arc::new(RecordingNetProvider::default());
    let mut doc = HtmlDocument::from_html(
        "<html><head></head><body></body></html>",
        DocumentConfig {
            base_url: Some("http://example.com/".to_string()),
            net_provider: Some(Arc::clone(&net) as _),
            ..Default::default()
        },
    );
    let head = doc
        .find_element_by_tag_name(&local_name!("head"))
        .expect("head")
        .id;
    let link = doc
        .mutate()
        .create_element(QualName::new(None, ns!(html), local_name!("link")), vec![]);
    doc.mutate().append_children(head, &[link]);
    assert!(net.requests.lock().unwrap().is_empty());
    (doc, link, net)
}

#[test]
fn rel_set_after_href_loads_the_sheet() {
    let (mut doc, link, net) = doc_with_link();

    doc.mutate().set_attribute(link, attr("href"), "late.css");
    assert!(
        net.requests.lock().unwrap().is_empty(),
        "an href alone does not name a stylesheet"
    );

    doc.mutate().set_attribute(link, attr("rel"), "stylesheet");
    assert_eq!(
        *net.requests.lock().unwrap(),
        vec!["http://example.com/late.css".to_string()]
    );
}

#[test]
fn href_set_after_rel_loads_the_sheet() {
    let (mut doc, link, net) = doc_with_link();

    doc.mutate().set_attribute(link, attr("rel"), "stylesheet");
    assert!(net.requests.lock().unwrap().is_empty());

    doc.mutate().set_attribute(link, attr("href"), "late.css");
    assert_eq!(
        *net.requests.lock().unwrap(),
        vec!["http://example.com/late.css".to_string()]
    );
}
