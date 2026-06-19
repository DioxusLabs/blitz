use std::sync::Arc;

use blitz_dom::{DocumentConfig, LocalName, QualName, ns};
use blitz_html::{HtmlDocument, HtmlProvider};

fn qname(local: &str) -> QualName {
    QualName {
        prefix: None,
        ns: ns!(html),
        local: LocalName::from(local),
    }
}

#[test]
fn detached_attribute_before_insertion_resolves_with_descendant_selector() {
    let mut doc = HtmlDocument::from_html(
        r#"<!doctype html><html><head>
<style>.page-header h1 { margin: 0 0 8px; }</style>
</head><body></body></html>"#,
        DocumentConfig {
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            ..Default::default()
        },
    );

    let body_id = doc.query_selector("body").unwrap().unwrap();
    let mut mutator = doc.mutate();

    let header = mutator.create_element(qname("header"), Vec::new());
    mutator.set_attribute(header, qname("class"), "page-header");

    let h1 = mutator.create_element(qname("h1"), Vec::new());
    let text = mutator.create_text_node("Title");
    mutator.append_children(h1, &[text]);
    mutator.append_children(header, &[h1]);
    mutator.append_children(body_id, &[header]);
    drop(mutator);

    doc.resolve(0.0);
}
