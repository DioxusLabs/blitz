//! When the viewport shrinks (a soft keyboard opening) while a text input is
//! focused, the input is scrolled back into view, as mobile browsers do.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn focused_doc(input_style: &str) -> HtmlDocument {
    let html = format!(
        r#"<!DOCTYPE html><html><body style="margin:0">
        <div style="height:500px"></div>
        <input id="field" style="display:block;height:20px;{input_style}">
        <div style="height:1000px"></div>
        </body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let field = doc.query_selector("#field").unwrap().unwrap();
    doc.set_focus_to(field);
    doc.resolve(0.0);
    doc
}

fn resize(doc: &mut HtmlDocument, height: u32) {
    doc.viewport_mut().window_size = (400, height);
    doc.resolve(0.0);
}

#[test]
fn shrinking_viewport_reveals_focused_input() {
    let mut doc = focused_doc("");
    assert_eq!(doc.viewport_scroll().y, 0.0);

    resize(&mut doc, 300);
    let field = doc.query_selector("#field").unwrap().unwrap();
    let node = doc.get_node(field).unwrap();
    let bottom = node.absolute_position(0.0, 0.0).y + node.final_layout().size.height;
    assert_eq!(doc.viewport_scroll().y, (bottom - 300.0) as f64);

    // The reveal runs once: later frames keep the user's own scrolling.
    doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 0.0 });
    doc.resolve(0.0);
    assert_eq!(doc.viewport_scroll().y, 0.0);
}

#[test]
fn growing_viewport_does_not_scroll() {
    let mut doc = focused_doc("");
    resize(&mut doc, 300);
    doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 0.0 });
    doc.resolve(0.0);

    resize(&mut doc, 400);
    assert_eq!(doc.viewport_scroll().y, 0.0);
}

#[test]
fn viewport_fixed_input_does_not_scroll() {
    let mut doc = focused_doc("position:fixed;top:500px");
    resize(&mut doc, 300);
    assert_eq!(doc.viewport_scroll().y, 0.0);
}
