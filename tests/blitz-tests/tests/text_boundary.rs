//! Reading text back through the text backend boundary, under whichever backend the build
//! selects: selected text, selection rectangles, the first baseline, and an `<input>` or
//! `<textarea>`'s metrics and caret.

use blitz_dom::node::TextLayout;
use blitz_dom::text::{Edit, EditableText as _, InlineText as _, Motion, TextEditor};
use blitz_dom::{BaseDocument, NodeId};
use blitz_test_harness::Harness;
use markup5ever::{QualName, local_name, ns};

fn page(body: &str) -> Harness {
    Harness::from_html(&format!(
        r#"<html><body style="margin:0; font: 16px/20px sans-serif">{body}</body></html>"#
    ))
}

fn inline<'a>(doc: &'a BaseDocument, selector: &str) -> &'a TextLayout {
    let id = doc.query_selector(selector).unwrap().unwrap();
    doc.get_node(id)
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_deref()
        .unwrap()
}

fn editor(doc: &BaseDocument, node: NodeId) -> &TextEditor {
    let element = doc.get_node(node).unwrap().element_data().unwrap();
    &element.text_input_data().unwrap().editor
}

#[test]
fn selected_text_borrows_the_laid_out_text() {
    let harness = page(r#"<div id="test">hello world</div>"#);
    let doc = harness.base();
    let layout = inline(&doc, "#test");
    let text = layout.text();
    let pieces: Vec<&str> = layout.selected_text(6, 11).collect();
    assert_eq!(pieces.concat(), "world");
    let within = text.as_bytes().as_ptr_range();
    for piece in pieces {
        assert!(within.contains(&piece.as_ptr()), "{piece:?} is a copy");
    }
    assert_eq!(layout.selected_text(4, text.len() + 1).count(), 0);
}

#[test]
fn selection_rects_cover_each_line() {
    let harness = page(r#"<div id="test">first<br>second</div>"#);
    let doc = harness.base();
    let layout = inline(&doc, "#test");
    let mut rects = Vec::new();
    layout.for_each_selection_rect(0, layout.text_len(), |rect| rects.push(rect));
    assert!(rects.iter().all(|rect| rect.width() > 0.0), "{rects:?}");
    // A backend may highlight a line's text in more than one rectangle.
    let mut tops: Vec<f64> = rects.iter().map(|rect| rect.y0).collect();
    tops.dedup();
    assert_eq!(tops.len(), 2, "{rects:?}");
    assert!(tops[0] < tops[1], "{rects:?}");
}

#[test]
fn first_baseline_is_on_the_first_line() {
    // Each line box is 40px tall, and the padding is outside the content box.
    let harness =
        page(r#"<div id="test" style="line-height: 40px; padding-top: 7px">a<br>b</div>"#);
    let doc = harness.base();
    let baseline = inline(&doc, "#test").first_baseline().unwrap();
    assert!(baseline > 12.0 && baseline < 40.0, "{baseline}");
}

#[test]
fn text_input_metrics_follow_its_value() {
    let mut harness = page(r#"<input id="field" value="hello" style="width:400px">"#);
    let node = harness.node("#field");
    let before = editor(&harness.base(), node).metrics().unwrap();
    assert_eq!(before.scale, 1.0);
    assert!(
        before.size.width > 0.0 && before.size.height > 0.0,
        "{before:?}"
    );

    // A value set from outside is laid out before it is read, and the caret follows it.
    let value = QualName::new(None, ns!(), local_name!("value"));
    harness
        .base_mut()
        .mutate()
        .set_attribute(node, value, "hello world");
    harness.base_mut().with_text_input(node, |mut driver| {
        driver.edit(Edit::Move(Motion::TextEnd));
    });
    let doc = harness.base();
    let editor = editor(&doc, node);
    let after = editor.metrics().unwrap();
    assert!(after.size.width > before.size.width, "{before:?} {after:?}");
    let caret = editor.caret_rect().unwrap();
    assert!(
        (caret.x0 - after.size.width).abs() < 2.0,
        "{caret:?} {after:?}"
    );
}

#[test]
fn textarea_wraps_at_its_width_once_laid_out() {
    let harness = page(
        r#"<textarea id="field" style="width:100px; height:200px">the quick brown fox jumps over the lazy dog</textarea>"#,
    );
    let node = harness.node("#field");
    let doc = harness.base();
    let metrics = editor(&doc, node).metrics().unwrap();
    assert!(metrics.size.width <= 100.0, "{metrics:?}");
    assert!(metrics.size.height > 40.0, "{metrics:?}");
}
