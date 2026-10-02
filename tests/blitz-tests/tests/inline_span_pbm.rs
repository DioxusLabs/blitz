//! Padding, border and margin on non-atomic inline elements (e.g. `<span>`): the
//! inline-axis sides take up space in the line, the block-axis sides only enlarge the box
//! that is painted and reported by `getClientRects`.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BoundingRect, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const WIDTH: u32 = 400;
const HEIGHT: u32 = 200;

fn make_doc(body: &str) -> HtmlDocument {
    let html = format!(
        r#"<!DOCTYPE html>
        <html><head><style>
            html {{ background: #fff; }}
            body {{ margin: 0; font-size: 20px; line-height: 40px; }}
            p {{ margin: 0; }}
        </style></head>
        <body>{body}</body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(WIDTH, HEIGHT, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn rects(doc: &HtmlDocument, selector: &str) -> Vec<BoundingRect> {
    let node_id = doc.query_selector(selector).unwrap().unwrap();
    doc.node_client_rects(node_id)
}

fn rect(doc: &HtmlDocument, selector: &str) -> BoundingRect {
    let rects = rects(doc, selector);
    assert_eq!(rects.len(), 1, "expected one fragment for {selector}");
    rects.into_iter().next().unwrap()
}

fn right(rect: &BoundingRect) -> f64 {
    rect.x + rect.width
}

#[track_caller]
fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 0.1,
        "expected {expected}, got {actual}"
    );
}

fn pixel(doc: &mut HtmlDocument, x: u32, y: u32) -> [u8; 3] {
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc, 1.0, WIDTH, HEIGHT, 0, 0),
        WIDTH,
        HEIGHT,
    );
    let idx = ((y * WIDTH + x) * 4) as usize;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

#[test]
fn inline_sides_take_up_space() {
    let plain =
        make_doc(r#"<p><span id="a">xx</span><span id="b">xx</span><span id="c">xx</span></p>"#);
    let doc = make_doc(
        r#"<p><span id="a">xx</span><span id="b" style="margin: 0 3px 0 5px; border-left: 2px solid; padding: 0 10px">xx</span><span id="c">xx</span></p>"#,
    );
    let text_width = rect(&plain, "#b").width;
    let (a, b, c) = (rect(&doc, "#a"), rect(&doc, "#b"), rect(&doc, "#c"));

    // The rect is the border box: margins are outside of it
    assert_close(b.x, right(&a) + 5.0);
    assert_close(b.width, text_width + 2.0 + 10.0 + 10.0);
    assert_close(c.x, right(&b) + 3.0);
}

#[test]
fn block_sides_do_not_affect_layout() {
    let plain = make_doc(r#"<p id="p">xx <span id="b">xx</span></p><p id="next">xx</p>"#);
    let doc = make_doc(
        r#"<p id="p">xx <span id="b" style="margin: 30px 0; border: solid; border-width: 4px 0; padding: 20px 0">xx</span></p><p id="next">xx</p>"#,
    );
    let (plain_b, b) = (rect(&plain, "#b"), rect(&doc, "#b"));

    assert_close(rect(&doc, "#next").y, rect(&plain, "#next").y);
    assert_close(b.x, plain_b.x);
    assert_close(b.width, plain_b.width);
    assert_close(b.y, plain_b.y - 24.0);
    assert_close(b.height, plain_b.height + 48.0);
}

#[test]
fn wrapped_span_has_its_sides_on_the_first_and_last_lines() {
    let body = |style: &str| {
        format!(
            r#"<p style="width: 60px">xx <span id="b" style="{style}">xxxx xxxx xxxx xxxx xxxx xxxx xxxx xxxx</span> xx</p>"#
        )
    };
    let plain = rects(&make_doc(&body("")), "#b");
    let padded = rects(&make_doc(&body("padding: 0 7px 0 4px")), "#b");

    assert!(plain.len() >= 3, "span should wrap: {plain:?}");
    assert_eq!(plain.len(), padded.len(), "{plain:?} {padded:?}");
    let last = plain.len() - 1;
    for (i, (plain, padded)) in plain.iter().zip(&padded).enumerate() {
        let sides = if i == 0 { 4.0 } else { 0.0 } + if i == last { 7.0 } else { 0.0 };
        assert_close(padded.x, plain.x);
        assert_close(padded.width, plain.width + sides);
    }
}

#[test]
fn sides_are_not_separated_from_the_content() {
    // The span's text fits on the first line but its padding does not, so the whole span
    // has to wrap rather than leaving a side behind.
    let body = |style: &str| {
        format!(
            r#"<p style="width: 150px"><span id="a">xx</span> <span id="b" style="{style}">xxxx</span></p>"#
        )
    };
    let plain = make_doc(&body(""));
    let padded = make_doc(&body("padding: 0 60px"));
    let text_width = rect(&plain, "#b").width;

    assert_close(rect(&plain, "#b").y, rect(&plain, "#a").y);
    let b = rect(&padded, "#b");
    assert_close(b.y, rect(&padded, "#a").y + 40.0);
    assert_close(b.x, 0.0);
    assert_close(b.width, text_width + 120.0);
}

#[test]
fn empty_span_has_a_box() {
    let doc = make_doc(
        r#"<p><span id="a">xx</span><span id="b" style="border: 1px solid; padding: 0 10px"></span><span id="c">xx</span></p>"#,
    );
    let (a, b, c) = (rect(&doc, "#a"), rect(&doc, "#b"), rect(&doc, "#c"));
    assert_close(b.x, right(&a));
    assert_close(b.width, 22.0);
    assert_close(c.x, right(&b));
    assert_close(b.height, a.height + 2.0);
}

#[test]
fn percentages_resolve_against_the_containing_block_width() {
    let body = |style: &str| {
        format!(
            r#"<p style="width: 200px"><span id="b" style="{style}">xx</span><span id="c">xx</span></p>"#
        )
    };
    let plain = make_doc(&body(""));
    let doc = make_doc(&body("padding-left: 10%; margin-right: 5%"));
    let b = rect(&doc, "#b");
    assert_close(b.width, rect(&plain, "#b").width + 20.0);
    assert_close(rect(&doc, "#c").x, right(&b) + 10.0);
}

#[test]
fn physical_sides_in_rtl() {
    let body = |style: &str| {
        format!(
            r#"<p dir="rtl" style="width: 300px"><span id="a">אא</span><span id="b" style="{style}">אא</span><span id="c">אא</span></p>"#
        )
    };
    let plain = make_doc(&body(""));
    let doc = make_doc(&body(
        "margin-left: 5px; padding-left: 10px; padding-right: 3px",
    ));
    let text_width = rect(&plain, "#b").width;
    let (a, b, c) = (rect(&doc, "#a"), rect(&doc, "#b"), rect(&doc, "#c"));

    // Right to left: a, then b to its left, then c
    assert_close(right(&a), 300.0);
    assert_close(right(&b), a.x);
    assert_close(b.width, text_width + 13.0);
    assert_close(right(&c), b.x - 5.0);
}

#[test]
fn logical_properties() {
    let body = |dir: &str| {
        format!(
            r#"<p dir="{dir}" style="width: 300px"><span id="b" style="margin-inline-start: 5px; padding-inline-end: 10px">אא</span><span id="c">אא</span></p>"#
        )
    };
    let ltr = make_doc(&body("ltr").replace("אא", "xx"));
    let (b, c) = (rect(&ltr, "#b"), rect(&ltr, "#c"));
    assert_close(b.x, 5.0);
    assert_close(c.x, right(&b));

    let rtl = make_doc(&body("rtl"));
    let (b, c) = (rect(&rtl, "#b"), rect(&rtl, "#c"));
    assert_close(right(&b), 295.0);
    assert_close(right(&c), b.x);
}

#[test]
fn background_and_border_are_painted_on_the_box() {
    let mut doc = make_doc(
        r#"<p><span style="color: transparent; background: #f00; border-left: 10px solid #00f; padding: 30px 0 30px 20px; margin-left: 10px">xx</span></p>"#,
    );
    // Along the middle of the line: margin, border, padding
    assert_eq!(pixel(&mut doc, 5, 20), WHITE);
    assert_eq!(pixel(&mut doc, 15, 20), BLUE);
    assert_eq!(pixel(&mut doc, 30, 20), RED);
    assert_eq!(pixel(&mut doc, 45, 20), RED);
    // The vertical padding overflows the 40px line box
    assert_eq!(pixel(&mut doc, 30, 55), RED);
    assert_eq!(pixel(&mut doc, 15, 55), BLUE);
}

#[test]
fn empty_span_is_painted() {
    let mut doc = make_doc(
        r#"<p id="p"><span style="background: #f00; padding: 0 10px; border-right: 10px solid #00f"></span></p>"#,
    );
    assert_eq!(pixel(&mut doc, 10, 20), RED);
    assert_eq!(pixel(&mut doc, 25, 20), BLUE);
    assert_eq!(pixel(&mut doc, 35, 20), WHITE);
    // The span takes up space, so its line box is not empty
    assert_close(rect(&doc, "#p").height, 40.0);
    let empty = make_doc(r#"<p id="p"><span></span></p>"#);
    assert_close(rect(&empty, "#p").height, 0.0);
}

#[test]
fn wrapped_span_is_only_bordered_at_its_ends() {
    let mut doc = make_doc(
        r#"<p style="width: 150px"><span id="b" style="color: transparent; background: #f00; border: solid #00f; border-width: 0 10px">xxxx xxxx xxxx xxxx xxxx</span></p>"#,
    );
    let fragments = rects(&doc, "#b");
    assert!(fragments.len() >= 3, "span should wrap: {fragments:?}");
    let last = fragments.len() - 1;
    for (i, fragment) in fragments.iter().enumerate() {
        let y = (fragment.y + fragment.height / 2.0) as u32;
        let left = pixel(&mut doc, 5, y);
        let right = pixel(&mut doc, (fragment.x + fragment.width) as u32 - 5, y);
        assert_eq!(left, if i == 0 { BLUE } else { RED }, "line {i}");
        assert_eq!(right, if i == last { BLUE } else { RED }, "line {i}");
    }
}
