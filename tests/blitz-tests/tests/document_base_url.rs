use blitz_dom::{DocumentConfig, qual_name};
use blitz_html::HtmlDocument;

#[test]
fn document_base_follows_the_first_connected_base_with_href() {
    let location = "https://example.test/page/index.html";
    let mut doc = HtmlDocument::from_html(
        r#"<base id="first" href="first/"><base id="second" href="second/"><div id="target"></div>"#,
        DocumentConfig {
            base_url: Some(location.into()),
            ..Default::default()
        },
    );
    let find = |doc: &HtmlDocument, selector| doc.query_selector(selector).unwrap().unwrap();
    let first = find(&doc, "#first");
    let second = find(&doc, "#second");
    let target = find(&doc, "#target");

    assert_eq!(doc.base_url().as_str(), "https://example.test/page/first/");
    {
        let mut dom = doc.mutate();
        dom.set_style_property(target, "background-image", "url(tile.png)");
        dom.set_attribute(first, qual_name!("href"), "changed/");
        dom.insert_nodes_before(first, &[second]);
    }
    assert_eq!(doc.base_url().as_str(), "https://example.test/page/second/");
    doc.resolve(0.0);
    assert!(
        doc.resolved_style_value(target, "background-image")
            .contains("/page/second/tile.png")
    );

    doc.mutate().clear_attribute(second, qual_name!("href"));
    assert_eq!(
        doc.base_url().as_str(),
        "https://example.test/page/changed/"
    );
    doc.mutate().remove_node(first);
    assert_eq!(doc.base_url().as_str(), location);
    assert_eq!(doc.url().as_str(), location);
}
