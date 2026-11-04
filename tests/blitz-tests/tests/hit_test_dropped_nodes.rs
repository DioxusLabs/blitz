//! Hit testing between a DOM mutation and the next `resolve` consults the
//! paint lists built by the previous `resolve`, which may still list nodes the
//! mutation dropped (e.g. a `position: fixed; z-index: 10` status bar hoisted
//! into the root stacking context).

use blitz_dom::{BaseDocument, DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn make_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn node_id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().expect(selector)
}

fn hit(doc: &BaseDocument, x: f32, y: f32) -> Option<NodeId> {
    doc.hit(x, y).map(|h| h.node_id)
}

#[test]
fn hit_test_skips_dropped_nodes() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html><html><body style="margin:0">
        <div id="content" style="position:relative;z-index:0;width:300px;height:300px">
            <!-- hoisted into `content`'s negative z-index list, sticking out of its box -->
            <div id="underlay" style="position:relative;left:320px;width:50px;height:50px;z-index:-1"></div>
            <!-- an ordinary paint child of `content` -->
            <div id="in-flow" style="width:100px;height:100px"></div>
            <!-- inline text, found through the inline root's inline layout -->
            <p id="paragraph" style="margin:0;font-size:20px;line-height:1"><span id="span">some text</span></p>
        </div>
        <!-- hoisted into the root stacking context's positive z-index list -->
        <div id="statusbar" style="position:fixed;left:0;bottom:0;width:200px;height:20px;z-index:10"></div>
        </body></html>"#,
    );
    let body = node_id(&doc, "body");
    let content = node_id(&doc, "#content");
    let underlay = node_id(&doc, "#underlay");
    let in_flow = node_id(&doc, "#in-flow");
    let paragraph = node_id(&doc, "#paragraph");
    let span = node_id(&doc, "#span");
    let statusbar = node_id(&doc, "#statusbar");

    assert_eq!(hit(&doc, 10.0, 290.0), Some(statusbar));
    assert_eq!(hit(&doc, 330.0, 10.0), Some(underlay));
    assert_eq!(hit(&doc, 10.0, 60.0), Some(in_flow));
    assert_eq!(hit(&doc, 10.0, 160.0), Some(span));

    let mut mutator = doc.mutate();
    mutator.remove_and_drop_node(statusbar);
    mutator.remove_and_drop_node(underlay);
    mutator.remove_and_drop_node(in_flow);
    mutator.remove_and_drop_node(span);
    drop(mutator);
    assert!(doc.get_node(statusbar).is_none());

    // Not yet resolved: the stale paint lists must be skipped over
    assert_eq!(hit(&doc, 10.0, 290.0), Some(content));
    assert_eq!(hit(&doc, 330.0, 10.0), Some(body));
    assert_eq!(hit(&doc, 10.0, 60.0), Some(content));
    assert_eq!(hit(&doc, 10.0, 160.0), Some(paragraph));
    doc.set_hover_to(10.0, 290.0);
    assert_eq!(doc.get_hover_node_id(), Some(content));

    doc.resolve(0.0);
    assert_eq!(hit(&doc, 10.0, 290.0), Some(content));
    assert_eq!(hit(&doc, 330.0, 10.0), Some(body));
    assert_eq!(hit(&doc, 10.0, 60.0), Some(content));
    assert_eq!(hit(&doc, 10.0, 160.0), Some(content));
}
