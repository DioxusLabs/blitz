use blitz_dom::{DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn make_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 200, 1.0, ColorScheme::Light)),
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

fn interaction_states(doc: &HtmlDocument, selector: &str) -> (bool, bool) {
    let node = doc.get_node(node_id(doc, selector)).unwrap();
    (node.is_hovered(), node.is_active())
}

#[test]
fn label_hover_delegates_to_implicit_and_explicit_inputs() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html><html><body style="margin:0">
        <label id="implicit" style="display:block;width:200px;height:40px">
          <span id="implicit-hit" style="display:inline-block;width:20px;height:20px"></span>
          <input id="implicit-input" style="position:absolute;left:300px;top:0">
        </label>
        <label id="explicit" for="explicit-input" style="display:block;width:200px;height:40px">
          <span id="explicit-hit" style="display:inline-block;width:20px;height:20px"></span>
        </label>
        <input id="explicit-input" style="position:absolute;left:300px;top:50px">
        </body></html>"#,
    );

    assert!(doc.set_hover_to(10.0, 10.0));
    assert!(interaction_states(&doc, "#implicit-input").0);

    assert!(doc.set_hover_to(10.0, 50.0));
    assert!(!interaction_states(&doc, "#implicit-input").0);
    assert!(interaction_states(&doc, "#explicit-input").0);
}

#[test]
fn label_active_delegates_to_implicit_and_explicit_inputs() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html><html><body style="margin:0">
        <label id="implicit" style="display:block;width:200px;height:40px">
          <span style="display:inline-block;width:20px;height:20px"></span>
          <input id="implicit-input" style="position:absolute;left:300px;top:0">
        </label>
        <label id="explicit" for="explicit-input" style="display:block;width:200px;height:40px">
          <span style="display:inline-block;width:20px;height:20px"></span>
        </label>
        <input id="explicit-input" style="position:absolute;left:300px;top:50px">
        </body></html>"#,
    );

    doc.set_hover_to(10.0, 10.0);
    assert!(doc.active_node());
    assert!(interaction_states(&doc, "#implicit-input").1);
    assert!(doc.unactive_node());
    assert!(!interaction_states(&doc, "#implicit-input").1);

    doc.set_hover_to(10.0, 50.0);
    assert!(doc.active_node());
    assert!(interaction_states(&doc, "#explicit-input").1);
    assert!(doc.unactive_node());
}

#[test]
fn label_delegated_states_do_not_flicker_between_descendants() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html><html><body style="margin:0">
        <label id="label" style="display:block;width:200px;height:40px">
          <span id="first" style="display:inline-block;width:40px;height:30px"></span><span
            id="second" style="display:inline-block;width:40px;height:30px"></span>
          <input id="input" style="position:absolute;left:300px;top:0">
        </label>
        </body></html>"#,
    );

    doc.set_hover_to(10.0, 10.0);
    doc.active_node();

    assert!(doc.set_hover_to(50.0, 10.0));
    assert_eq!(interaction_states(&doc, "#input"), (true, true));
    assert_eq!(interaction_states(&doc, "#label"), (true, true));

    assert!(doc.unactive_node());
    assert_eq!(interaction_states(&doc, "#input"), (true, false));
    assert!(doc.set_hover_to(10.0, 100.0));
    assert_eq!(interaction_states(&doc, "#input"), (false, false));
}
