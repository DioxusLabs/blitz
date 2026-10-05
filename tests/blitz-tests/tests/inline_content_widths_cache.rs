use blitz_dom::{DocumentConfig, node::TextLayout};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    node_id::NodeId,
    shell::{ColorScheme, Viewport},
};
use std::sync::Arc;

fn make_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 400, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().unwrap()
}

fn inline_layout(doc: &HtmlDocument, node_id: NodeId) -> &TextLayout {
    doc.get_node(node_id)
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .unwrap()
}

fn cached_widths(doc: &HtmlDocument, node_id: NodeId) -> (f32, f32) {
    let layout = inline_layout(doc, node_id);
    assert_eq!(layout.layout.inline_boxes().len(), 0);
    let cached = layout.content_widths.unwrap();
    let actual = layout.layout.calculate_content_widths();
    assert_eq!((cached.min, cached.max), (actual.min, actual.max));
    (cached.min, cached.max)
}

#[test]
fn text_only_widths_are_cached_and_reused() {
    let mut doc = make_doc(r#"<div id="c" style="width:max-content">hello world</div>"#);
    let c = id(&doc, "#c");
    let widths = cached_widths(&doc, c);
    assert!(widths.0 > 0.0 && widths.1 > widths.0);
    doc.resolve(0.0);
    assert_eq!(cached_widths(&doc, c), widths);

    doc.get_node_mut(c).unwrap().clear_layout_cache();
    assert_eq!(cached_widths(&doc, c), widths);
    doc.resolve(0.0);
    assert_eq!(cached_widths(&doc, c), widths);

    doc.get_node_mut(c).unwrap().invalidate_layout_cache();
    assert!(inline_layout(&doc, c).content_widths.is_none());
    doc.get_node_mut(c)
        .unwrap()
        .element_data_mut()
        .unwrap()
        .inline_layout_data
        .as_mut()
        .unwrap()
        .content_widths();
    assert_eq!(cached_widths(&doc, c), widths);
}

#[test]
fn inline_box_widths_are_not_cached() {
    let mut doc = make_doc(
        r#"<div id="c" style="width:max-content">hello <span style="display:inline-block;width:50px">world</span></div>"#,
    );
    let c = id(&doc, "#c");
    let layout = inline_layout(&doc, c);
    assert_eq!(layout.layout.inline_boxes().len(), 1);
    assert!(layout.content_widths.is_none());
    let layout = doc
        .get_node_mut(c)
        .unwrap()
        .element_data_mut()
        .unwrap()
        .inline_layout_data
        .as_mut()
        .unwrap();
    let before = layout.content_widths();
    layout.layout.inline_boxes_mut().next().unwrap().width += 100.0;
    let after = layout.content_widths();
    assert_eq!(after.max, before.max + 100.0);
    assert!(layout.content_widths.is_none());
}

#[test]
fn text_mutation_rebuilds_cached_widths() {
    let mut doc = make_doc(r#"<div id="c" style="width:max-content">hello</div>"#);
    let c = id(&doc, "#c");
    let before = cached_widths(&doc, c);
    let text = doc.get_node(c).unwrap().children[0];
    doc.mutate().set_node_text(text, "hello wonderful world");
    doc.resolve(0.0);
    let after = cached_widths(&doc, c);
    assert!(after.0 > before.0 && after.1 > before.1);
    assert_eq!(
        doc.get_node(c).unwrap().final_layout().size.width,
        after.1.ceil()
    );
}

#[test]
fn descendant_restyle_rebuilds_cached_widths_in_anonymous_block() {
    let mut doc = make_doc(
        r#"<style>body{margin:0} #c{width:max-content;font-size:10px} #s:hover{font-size:20px}</style><div id="c"><span id="s">hello world</span><div></div></div>"#,
    );
    let c = id(&doc, "#c");
    let s = id(&doc, "#s");
    let anon = doc.get_node(s).unwrap().layout_parent.get().unwrap();
    assert!(doc.get_node(anon).unwrap().is_anonymous());
    let before = cached_widths(&doc, anon);
    assert!(doc.set_hover_to(5.0, 5.0));
    assert_eq!(doc.get_hover_node_id(), Some(s));
    doc.resolve(0.0);
    let anon = doc.get_node(s).unwrap().layout_parent.get().unwrap();
    let after = cached_widths(&doc, anon);
    assert_eq!(after, (before.0 * 2.0, before.1 * 2.0));
    assert_eq!(
        doc.get_node(c).unwrap().final_layout().size.width,
        after.1.ceil()
    );
}

#[test]
fn scale_change_rebuilds_cached_widths() {
    let mut doc = make_doc(r#"<div id="c" style="width:max-content">hello world</div>"#);
    let c = id(&doc, "#c");
    let before = cached_widths(&doc, c);
    doc.set_viewport(Viewport::new(800, 800, 2.0, ColorScheme::Light));
    doc.resolve(0.0);
    let after = cached_widths(&doc, c);
    assert_eq!(after, (before.0 * 2.0, before.1 * 2.0));
    assert_eq!(
        doc.get_node(c).unwrap().final_layout().size.width,
        before.1.ceil()
    );
}

#[test]
fn relayout_damage_clears_cached_widths_without_rebuilding_text() {
    let mut doc = make_doc(
        r#"<style>body{margin:0} #c{width:max-content} #c:hover{padding-left:10px}</style><div id="c">hello world</div>"#,
    );
    let c = id(&doc, "#c");
    let before = cached_widths(&doc, c);
    let layout = doc
        .get_node_mut(c)
        .unwrap()
        .element_data_mut()
        .unwrap()
        .inline_layout_data
        .as_mut()
        .unwrap();
    layout.content_widths.as_mut().unwrap().max = 1.0;
    let text_ptr = layout.text.as_ptr();
    assert!(doc.set_hover_to(5.0, 5.0));
    doc.resolve(0.0);
    assert_eq!(inline_layout(&doc, c).text.as_ptr(), text_ptr);
    assert_eq!(cached_widths(&doc, c), before);
    assert_eq!(
        doc.get_node(c).unwrap().final_layout().size.width,
        (before.1 + 10.0).ceil()
    );
}
