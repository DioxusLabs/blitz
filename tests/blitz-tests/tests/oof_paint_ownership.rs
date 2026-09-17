//! Paint ownership of hoisted out-of-flow boxes: an absolutely/fixed
//! positioned box takes its *geometry* from its containing block but is
//! *painted* from the nearest DOM ancestor stacking context (CSS 2.1
//! Appendix E), even when that ancestor is not its containing block.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{DocumentConfig, ScrollBehavior};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use markup5ever::{QualName, local_name, ns};
use std::sync::Arc;

const W: u32 = 300;
const H: u32 = 300;

const RED: [u8; 4] = [255, 0, 0, 255];
const LIME: [u8; 4] = [0, 255, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
/// Red at 20% opacity over white
const PINK: [u8; 4] = [255, 204, 204, 255];
const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];

fn document(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(W, H, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn render(doc: &mut HtmlDocument) -> Vec<u8> {
    doc.resolve(0.0);
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc, 1.0, W, H, 0, 0),
        W,
        H,
    )
}

fn pixel(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

fn set_style(doc: &mut HtmlDocument, selector: &str, style: &str) {
    let id = doc.query_selector(selector).unwrap().unwrap();
    doc.mutate()
        .set_attribute(id, QualName::new(None, ns!(), local_name!("style")), style);
}

#[test]
fn fixed_inside_negative_z_ancestor_paints_in_that_stacking_context() {
    // The fixed box's containing block is the root, but it is a descendant of
    // a `z-index: -1` stacking context, so it paints below the in-flow lime box.
    let mut doc = document(
        r#"<body style="margin:0">
        <div style="position:relative;z-index:-1">
            <div style="position:fixed;left:10px;top:10px;width:80px;height:80px;background:red"></div>
        </div>
        <div style="width:200px;height:200px;background:lime"></div>"#,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 50, 50), LIME);
}

#[test]
fn z_indexed_fixed_inside_positive_z_ancestor_paints_in_that_stacking_context() {
    // z-index: 5 is resolved within the `z-index: 1` ancestor's stacking
    // context, which as a whole paints below the `z-index: 2` sibling.
    let mut doc = document(
        r#"<body style="margin:0">
        <div style="position:relative;z-index:1">
            <div style="position:fixed;left:10px;top:10px;width:80px;height:80px;background:red;z-index:5"></div>
        </div>
        <div style="position:relative;z-index:2;width:200px;height:200px;background:lime"></div>"#,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 50, 50), LIME);
}

#[test]
fn z_indexed_abspos_inside_opacity_ancestor_is_painted_with_its_opacity() {
    let mut doc = document(
        r#"<body style="margin:0">
        <div style="position:relative;width:300px;height:200px;background:#fff">
            <div style="opacity:0.2;width:100px;height:100px;background:blue">
                <div style="position:absolute;left:150px;top:20px;width:60px;height:60px;z-index:1;background:red"></div>
            </div>
        </div>"#,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 180, 50), PINK);
}

#[test]
fn abspos_inside_opacity_ancestor_is_painted_with_its_opacity_outside_its_border_box() {
    let mut doc = document(
        r#"<body style="margin:0">
        <div style="position:relative;width:300px;height:200px;background:#fff">
            <div style="opacity:0.2;width:100px;height:100px;background:blue">
                <div style="position:absolute;left:150px;top:20px;width:60px;height:60px;background:red"></div>
            </div>
        </div>"#,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 180, 50), PINK);
    let hit = doc.hit(180.0, 50.0).unwrap();
    let abspos = doc
        .query_selector("[style*='position:absolute']")
        .unwrap()
        .unwrap();
    assert_eq!(hit.node_id, abspos);
}

#[test]
fn abspos_is_not_clipped_by_non_positioned_overflow_hidden_ancestor() {
    let mut doc = document(
        r#"<body style="margin:0">
        <div style="overflow:hidden;width:100px;height:100px;background:#ccc">
            <div style="position:absolute;left:150px;top:20px;width:60px;height:60px;background:red"></div>
        </div>"#,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 180, 50), RED);
}

#[test]
fn abspos_is_clipped_by_overflow_hidden_containing_block_ancestor() {
    let mut doc = document(
        r#"<body style="margin:0">
        <div style="overflow:hidden;width:100px;height:100px;background:#ccc">
            <div style="position:relative">
                <div style="position:absolute;left:80px;top:20px;width:60px;height:60px;background:red"></div>
            </div>
        </div>"#,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 90, 50), RED);
    assert_eq!(pixel(&buf, 120, 50), TRANSPARENT);
}

#[test]
fn fixed_boxes_do_not_scroll_with_the_viewport() {
    for z_index in ["auto", "10"] {
        let mut doc = document(&format!(
            r#"<body style="margin:0"><div style="height:2000px"></div>
            <div style="position:fixed;left:10px;top:10px;width:80px;height:80px;background:red;z-index:{z_index}"></div>"#,
        ));
        doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 100.0 });
        let buf = render(&mut doc);
        assert_eq!(pixel(&buf, 50, 50), RED, "z-index: {z_index}");
        assert_eq!(pixel(&buf, 50, 150), TRANSPARENT, "z-index: {z_index}");
        // Hit-testing takes document coordinates
        let hit = doc.hit(50.0, 150.0).unwrap();
        let fixed = doc
            .query_selector("[style*='position:fixed']")
            .unwrap()
            .unwrap();
        assert_eq!(hit.node_id, fixed, "z-index: {z_index}");
    }
}

#[test]
fn fixed_inside_transformed_scroller_scrolls_with_it() {
    // A transformed ancestor is the containing block for fixed descendants,
    // which then behave like absolutely positioned boxes.
    let mut doc = document(
        r#"<body style="margin:0">
        <div id=s style="transform:translateX(0);overflow:scroll;width:200px;height:200px">
            <div style="height:2000px"></div>
            <div style="position:fixed;left:10px;top:10px;width:80px;height:80px;background:red"></div>
        </div>"#,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 50, 50), RED);

    let scroller = doc.query_selector("#s").unwrap().unwrap();
    doc.scroll_to(scroller, 0.0, 100.0, ScrollBehavior::Instant);
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 50, 50), TRANSPARENT);
}

#[test]
fn hoisted_box_follows_containing_block_changes_incrementally() {
    // `#target` is hoisted past `#mid` to `#outer`. Making `#mid` positioned
    // moves the containing block (and so the painted position) to `#mid`;
    // both the paint lists of `#mid` and `#outer` must be rebuilt.
    let mut doc = document(
        r#"<body style="margin:0">
        <div id=outer style="position:relative;width:300px;height:300px;background:#fff">
            <div style="height:100px"></div>
            <div id=mid style="margin-left:100px;width:100px;height:100px;background:#ccc">
                <div id=target style="position:absolute;left:0;top:0;width:50px;height:50px;background:red"></div>
            </div>
        </div>"#,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 25), RED);
    assert_eq!(pixel(&buf, 125, 125), [204, 204, 204, 255]);

    set_style(
        &mut doc,
        "#mid",
        "margin-left:100px;width:100px;height:100px;background:#ccc;position:relative",
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 25), WHITE);
    assert_eq!(pixel(&buf, 125, 125), RED);
    let target = doc.query_selector("#target").unwrap().unwrap();
    assert_eq!(doc.hit(125.0, 125.0).unwrap().node_id, target);

    set_style(
        &mut doc,
        "#mid",
        "margin-left:100px;width:100px;height:100px;background:#ccc",
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 25), RED);
    assert_eq!(pixel(&buf, 125, 125), [204, 204, 204, 255]);
    assert_eq!(doc.hit(25.0, 25.0).unwrap().node_id, target);
}
