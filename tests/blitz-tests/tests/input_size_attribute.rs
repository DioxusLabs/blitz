//! A single-line text input is sized from its `size` attribute, so changing the attribute after
//! the first layout must lay the input out again. (WPT's html/rendering/widgets/input-text-size.html
//! covers the static sizes; it never changes the attribute.)

use blitz_dom::{DocumentConfig, NodeId, QualName, local_name, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn size_attr() -> QualName {
    QualName::new(None, ns!(), local_name!("size"))
}

fn width(doc: &HtmlDocument, node: NodeId) -> f32 {
    doc.get_node(node).unwrap().final_layout().size.width
}

#[test]
fn changing_size_relayouts_the_input() {
    let mut doc = layout_doc(
        r#"<body style="margin:0; font-size:10px">
            <div><input id="t" size="5" style="border:0; padding:0"></div>
            <div><input id="default" size="20" style="border:0; padding:0"></div>
        </body>"#,
    );
    let node = doc.query_selector("#t").unwrap().unwrap();
    let default = doc.query_selector("#default").unwrap().unwrap();
    let narrow = width(&doc, node);
    let default_width = width(&doc, default);
    assert!(narrow < default_width, "size=5 is narrower than size=20");

    doc.mutate().set_attribute(node, size_attr(), "40");
    doc.resolve(0.0);
    assert!(
        width(&doc, node) > default_width,
        "setting size=40 after the first layout widens the input past size=20"
    );

    doc.mutate().clear_attribute(node, size_attr());
    doc.resolve(0.0);
    assert_eq!(
        width(&doc, node),
        default_width,
        "removing size falls back to the default of 20"
    );
}
