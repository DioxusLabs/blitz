//! An element that stops being a layout container and is laid out as part of inline content
//! must give up the layout children and anonymous blocks it was built with.
//!
//! Regression test: a flex container with an `::after` turned into a plain inline element,
//! which drops the pseudo-element from the slab, kept both in its layout children. The
//! layout_parent consistency check found a layout child that was not in the slab.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use markup5ever::{QualName, local_name, ns};
use std::sync::Arc;

// The link is a flex container while the list carries its id: an anonymous block around its
// text and an `::after` filling the rest of the line.
const HTML: &str = r##"<html><head><style>
    #list li { display: flex }
    #list a { display: flex; flex: 1 }
    #list a::after { content: ""; flex: 1 0 2em; border-bottom: 1px dotted }
</style></head><body>
    <ol id="list"><li><a id="link" href="#x">Title</a><span>3</span></li></ol>
</body></html>"##;

#[test]
fn an_element_laid_out_inline_drops_what_it_was_built_with_as_a_container() {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let link = doc.get_element_by_id("link").unwrap();
    assert!(doc.get_node(link).unwrap().after().is_some());
    assert!(!doc.get_node(link).unwrap().anonymous_blocks.is_empty());

    // Without the id, nothing makes the link a flex container or gives it an `::after`.
    let list = doc.get_element_by_id("list").unwrap();
    doc.mutate()
        .clear_attribute(list, QualName::new(None, ns!(), local_name!("id")));
    doc.resolve(0.0);

    let node = doc.get_node(link).unwrap();
    assert!(node.after().is_none());
    assert!(node.layout_children.borrow().is_none());
    assert!(node.anonymous_blocks.is_empty());
    for (node_id, node) in doc.tree().iter() {
        for child_id in node.layout_children.borrow().iter().flatten() {
            assert!(
                doc.get_node(*child_id).is_some(),
                "layout child {child_id:?} of {node_id:?} is not in the slab"
            );
        }
    }
}
