use std::cmp::Ordering;

use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;

#[test]
fn pseudo_elements_order_around_children() {
    let mut doc = HtmlDocument::from_html(
        r#"<!doctype html><style>#p::before { content: "a" } #p::after { content: "b" }</style>
<div id="p"><span id="c">x</span></div>"#,
        DocumentConfig::default(),
    );
    doc.resolve(0.0);

    let parent = doc.query_selector("#p").unwrap().unwrap();
    let child = doc.query_selector("#c").unwrap().unwrap();
    let before = doc.get_node(parent).unwrap().before().unwrap();
    let after = doc.get_node(parent).unwrap().after().unwrap();

    assert_eq!(doc.compare_document_order(before, child), Ordering::Less);
    assert_eq!(doc.compare_document_order(child, before), Ordering::Greater);
    assert_eq!(doc.compare_document_order(after, child), Ordering::Greater);
    assert_eq!(doc.compare_document_order(child, after), Ordering::Less);
    assert_eq!(doc.compare_document_order(before, after), Ordering::Less);
}
