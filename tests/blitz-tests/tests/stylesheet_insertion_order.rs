//! Author stylesheets cascade in document order: on equal specificity the sheet whose
//! `<style>`/`<link>` node comes later in tree order wins (CSS Cascade 4 § 6.4). A
//! dynamically inserted sheet must therefore be placed in the cascade by its node's tree
//! position (not by its `NodeId`, whose slab slot may be reused), and only relative to
//! sheets of the same origin.

use blitz_dom::{BaseDocument, CopyStyleGetters, DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use markup5ever::{QualName, local_name, ns};
use std::sync::Arc;
use style::stylesheets::Origin;

fn load(html: &str) -> BaseDocument {
    let config = DocumentConfig {
        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
        ..Default::default()
    };
    let mut doc = HtmlDocument::from_html(html, config).into_inner();
    doc.resolve(0.0);
    doc
}

fn query(doc: &BaseDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().unwrap()
}

/// The (r, g, b) components of the computed `color` of the element matching `selector`, in 0..=255.
fn color(doc: &BaseDocument, selector: &str) -> [u8; 3] {
    let id = query(doc, selector);
    let color = doc
        .get_node(id)
        .unwrap()
        .primary_styles()
        .unwrap()
        .clone_color();
    let c = color.raw_components();
    [c[0], c[1], c[2]].map(|v| (v * 255.0).round() as u8)
}

/// Create a detached `<style>` element containing `css`
fn create_style(doc: &mut BaseDocument, css: &str) -> NodeId {
    let mut mutator = doc.mutate();
    let style =
        mutator.create_element(QualName::new(None, ns!(html), local_name!("style")), vec![]);
    let text = mutator.create_text_node(css);
    mutator.append_children(style, &[text]);
    style
}

#[test]
fn appended_style_in_reused_slot_wins() {
    let mut doc = load(
        "<html><head><style>#t{color:rgb(1,1,1)}</style></head><body>\
         <div id=rm1><span></span><span></span><span></span><span></span></div>\
         <div id=rm2><i></i><i></i><i></i></div><div id=rm3><i></i><i></i></div>\
         <style>#t{color:rgb(3,3,3)}</style><p id=t>t</p></body></html>",
    );
    assert_eq!(color(&doc, "#t"), [3, 3, 3]);
    let body_style = query(&doc, "body > style");

    // Free some low node slots so that the new `<style>` reuses one of them
    for selector in ["#rm1", "#rm2", "#rm3"] {
        let id = query(&doc, selector);
        doc.mutate().remove_and_drop_node(id);
    }

    let body = query(&doc, "body");
    let new_style = create_style(&mut doc, "#t{color:rgb(2,2,2)}");
    doc.mutate().append_children(body, &[new_style]);
    doc.resolve(0.0);

    assert!(
        new_style < body_style,
        "test setup: expected the new <style> ({new_style:?}) to reuse a slot below the \
         existing body <style> ({body_style:?})"
    );
    assert_eq!(
        color(&doc, "#t"),
        [2, 2, 2],
        "the last <style> in the document ({new_style:?}) should win over the earlier \
         body <style> ({body_style:?})"
    );
}

#[test]
fn style_inserted_before_existing_style_loses() {
    let mut doc = load(
        "<html><head><style>#t{color:rgb(1,1,1)}</style></head>\
         <body><p id=t>t</p></body></html>",
    );
    assert_eq!(color(&doc, "#t"), [1, 1, 1]);

    let head = query(&doc, "head");
    let new_style = create_style(&mut doc, "#t{color:rgb(2,2,2)}");
    doc.mutate().prepend_nodes(head, &[new_style]);
    doc.resolve(0.0);
    assert_eq!(color(&doc, "#t"), [1, 1, 1]);

    // A later sibling of the existing `<style>` wins
    let body = query(&doc, "body");
    let later_style = create_style(&mut doc, "#t{color:rgb(4,4,4)}");
    doc.mutate().prepend_nodes(body, &[later_style]);
    doc.resolve(0.0);
    assert_eq!(color(&doc, "#t"), [4, 4, 4]);
}

#[test]
fn user_agent_sheet_for_earlier_node_does_not_panic() {
    let mut doc = load(
        "<html><body><div id=a></div><style>#t{color:rgb(3,3,3)}</style>\
         <p id=t>t</p><p id=u>u</p></body></html>",
    );
    let a = query(&doc, "#a");
    let author_style = query(&doc, "style");
    assert!(
        a < author_style,
        "test setup: {a:?} should be below {author_style:?}"
    );

    // The `before` sheet must be looked up among sheets of the same origin, else Stylo
    // panics with "`before_sheet` stylesheet not found".
    let sheet = doc.make_stylesheet(
        "#t{color:rgb(9,9,9)} #u{color:rgb(5,5,5)}",
        Origin::UserAgent,
    );
    doc.add_stylesheet_for_node(sheet, a);
    doc.resolve(0.0);

    // The UA sheet applies, but cascades below the author sheet
    assert_eq!(color(&doc, "#u"), [5, 5, 5]);
    assert_eq!(color(&doc, "#t"), [3, 3, 3]);
}
