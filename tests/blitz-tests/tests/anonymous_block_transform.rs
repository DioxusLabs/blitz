//! A `transform` change on an element wrapped in an anonymous block box must
//! be resolved even though the anonymous box itself carries no damage.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{ColorScheme, Viewport};
use markup5ever::{LocalName, QualName, ns};
use std::sync::Arc;

fn make_doc(incremental: bool) -> HtmlDocument {
    let html = r#"<!DOCTYPE html><html><head><style>
        body { margin: 0; }
        #target { display: inline-block; width: 50px; height: 50px; background: red; }
        #target:hover { transform: translateX(100px); }
        </style></head><body><span id="target"></span><div>block</div></body></html>"#;
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 400, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.set_incremental_layout(incremental);
    doc.resolve(0.0);
    doc
}

fn target(doc: &HtmlDocument) -> NodeId {
    doc.query_selector("#target").unwrap().unwrap()
}

fn translate_x(doc: &HtmlDocument) -> f64 {
    let node = doc.get_node(target(doc)).unwrap();
    node.transform()
        .as_deref()
        .map(|t| t.as_coeffs()[4])
        .unwrap_or(0.0)
}

fn assert_wrapped_in_anonymous_block(doc: &HtmlDocument) {
    let node = doc.get_node(target(doc)).unwrap();
    let parent = doc.get_node(node.layout_parent.get().unwrap()).unwrap();
    assert!(parent.is_anonymous());
}

#[test]
fn hover_transform_inside_anonymous_block() {
    for incremental in [false, true] {
        let mut doc = make_doc(incremental);
        assert_wrapped_in_anonymous_block(&doc);
        assert_eq!(translate_x(&doc), 0.0);

        assert!(doc.set_hover_to(10.0, 10.0));
        assert_eq!(doc.get_hover_node_id(), Some(target(&doc)));
        doc.resolve(0.0);
        assert_wrapped_in_anonymous_block(&doc);
        assert_eq!(translate_x(&doc), 100.0, "incremental={incremental}");
    }
}

#[test]
fn style_attribute_transform_inside_anonymous_block() {
    for incremental in [false, true] {
        let mut doc = make_doc(incremental);
        let id = target(&doc);
        doc.mutate().set_attribute(
            id,
            QualName::new(None, ns!(), LocalName::from("style")),
            "transform: translateX(30px)",
        );
        doc.resolve(0.0);
        assert_wrapped_in_anonymous_block(&doc);
        assert_eq!(translate_x(&doc), 30.0, "incremental={incremental}");
    }
}
