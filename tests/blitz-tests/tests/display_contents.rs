//! display:contents — children must lay out as if they were children of the
//! contents element's parent.

use blitz_dom::node::TextLayout;
use blitz_dom::{BaseDocument, DocumentConfig, Node};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::node_id::NodeId;
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

#[test]
fn contents_block_children_stack_and_fill_width() {
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div style="width:300px;">
                <div style="display:contents;">
                    <div id="a" style="height:50px;"></div>
                    <div id="b" style="height:70px;"></div>
                </div>
            </div>
        </body></html>"#,
    );
    let a = doc.query_selector("#a").unwrap().expect("#a not found");
    let b = doc.query_selector("#b").unwrap().expect("#b not found");
    let la = doc.get_node(a).unwrap().final_layout();
    let lb = doc.get_node(b).unwrap().final_layout();
    assert_eq!(
        (la.size.width, la.size.height),
        (300.0, 50.0),
        "first hoisted block child must fill the grandparent width"
    );
    assert_eq!(
        (lb.size.width, lb.size.height),
        (300.0, 70.0),
        "second hoisted block child must fill the grandparent width"
    );
    assert_eq!(
        lb.location.y - la.location.y,
        50.0,
        "hoisted block children must stack vertically"
    );
}

#[test]
fn scroller_wrapping_contents_gets_full_scroll_size() {
    // The kopuz route-shell shape: a scroll container whose only child is a
    // display:contents wrapper holding tall content.
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div id="scroller" style="height:200px; overflow-y:auto;">
                <div style="display:contents;">
                    <div id="tall" style="height:1000px;"></div>
                </div>
            </div>
        </body></html>"#,
    );
    let scroller = doc
        .query_selector("#scroller")
        .unwrap()
        .expect("#scroller not found");
    let tall = doc
        .query_selector("#tall")
        .unwrap()
        .expect("#tall not found");
    let ls = doc.get_node(scroller).unwrap().final_layout();
    let lt = doc.get_node(tall).unwrap().final_layout();
    assert_eq!(
        (lt.size.width, lt.size.height),
        (800.0, 1000.0),
        "content inside the contents wrapper must size normally"
    );
    assert_eq!(
        ls.scrollable_overflow_rect.bottom(),
        1000.0,
        "the scroller's scrollable content size must include the hoisted content"
    );
}

#[test]
fn contents_in_flex_container_hoists_children_as_flex_items() {
    // The kopuz showcase shape: flex rows whose direct children are
    // display:contents wrappers (keyed row wrappers).
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div style="display:flex; flex-direction:column; width:300px;">
                <div style="display:contents;">
                    <div id="x" style="height:40px;"></div>
                    <div id="y" style="height:40px;"></div>
                </div>
            </div>
        </body></html>"#,
    );
    let x = doc.query_selector("#x").unwrap().expect("#x not found");
    let y = doc.query_selector("#y").unwrap().expect("#y not found");
    let lx = doc.get_node(x).unwrap().final_layout();
    let ly = doc.get_node(y).unwrap().final_layout();
    assert_eq!(
        (lx.size.width, lx.size.height),
        (300.0, 40.0),
        "hoisted flex item must stretch to the flex container width"
    );
    assert_eq!(
        ly.location.y - lx.location.y,
        40.0,
        "hoisted flex items must stack in the column"
    );
}

#[test]
fn abspos_child_hoisted_through_contents_stretches() {
    // The kopuz route-shell chain: scroll container > display:contents shell
    // > absolute inset:0 page (plus a comment placeholder sibling). The page
    // must stretch to the scroll container's padding box.
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:300px; height:200px; overflow-y:auto;">
                <!-- placeholder -->
                <div style="display:contents;">
                    <div id="page" style="position:absolute; inset:0;">
                        <div style="height:5000px;"></div>
                    </div>
                </div>
            </div>
        </body></html>"#,
    );
    let page = doc
        .query_selector("#page")
        .unwrap()
        .expect("#page not found");
    let layout = doc.get_node(page).unwrap().final_layout();
    assert_eq!(
        (layout.size.width, layout.size.height),
        (300.0, 200.0),
        "abspos hoisted through display:contents must stretch to the containing block"
    );
}

/// Find the inline layout (possibly owned by an anonymous block) whose text
/// contains `needle`.
fn inline_layout_containing<'a>(doc: &'a BaseDocument, needle: &str) -> &'a TextLayout {
    fn has_text(node: &Node, needle: &str) -> Option<NodeId> {
        let layout = node.element_data()?.inline_layout_data.as_deref()?;
        layout.text.contains(needle).then_some(node.id)
    }

    let mut found = None;
    doc.visit(|_, node| {
        found = found.or_else(|| has_text(node, needle));
        for anonymous_id in node.anonymous_blocks.iter() {
            if let Some(anonymous) = doc.get_node(*anonymous_id) {
                found = found.or_else(|| has_text(anonymous, needle));
            }
        }
    });
    let id = found.unwrap_or_else(|| panic!("no inline layout containing {needle:?}"));
    doc.get_node(id)
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .unwrap()
}

fn assert_run_inherits(
    layout: &TextLayout,
    needle: &str,
    expected_node: NodeId,
    expected_font_size: f32,
) {
    let start = layout.text.find(needle).unwrap_or_else(|| {
        panic!("text {:?} does not contain {needle:?}", layout.text);
    });
    let end = start + needle.len();
    let mut saw_run = false;
    for line in layout.layout.lines() {
        for run in line.runs() {
            let range = run.text_range();
            if range.start >= end || range.end <= start {
                continue;
            }
            saw_run = true;
            assert!(
                (run.font_size() - expected_font_size).abs() < 0.01,
                "text {needle:?} run font size is {}, expected {expected_font_size}",
                run.font_size()
            );
            for cluster in run.clusters() {
                let range = cluster.text_range();
                if range.start >= end || range.end <= start {
                    continue;
                }
                assert_eq!(
                    cluster.style().brush.id,
                    expected_node,
                    "text {needle:?} must use the display:contents element's style"
                );
            }
        }
    }
    assert!(saw_run, "no run found for text {needle:?}");
}

#[test]
fn contents_text_inherits_styles_in_inline_layout() {
    let doc = layout_doc(
        r#"<html><body style="margin:0; color:black; font-size:16px">
            <div id="test"><span id="c" style="display:contents; color:red; font-size:24px; letter-spacing:4px">contents text</span></div>
        </body></html>"#,
    );
    let c = doc.query_selector("#c").unwrap().expect("#c not found");
    let layout = inline_layout_containing(&doc, "contents text");
    assert_run_inherits(layout, "contents text", c, 24.0);
}

#[test]
fn contents_text_inherits_styles_when_hoisted_into_anonymous_block() {
    let doc = layout_doc(
        r#"<html><body style="margin:0; color:black; font-size:16px">
            <div>
                <span id="c" style="display:contents; color:red; font-size:24px">hoisted text<div>block child</div>tail</span>
            </div>
        </body></html>"#,
    );
    let c = doc.query_selector("#c").unwrap().expect("#c not found");
    assert_run_inherits(
        inline_layout_containing(&doc, "hoisted text"),
        "hoisted text",
        c,
        24.0,
    );
    assert_run_inherits(inline_layout_containing(&doc, "tail"), "tail", c, 24.0);
}

#[test]
fn contents_text_inherits_styles_in_flex_and_grid_containers() {
    for display in ["flex", "grid"] {
        let doc = layout_doc(&format!(
            r#"<html><body style="margin:0; color:black; font-size:16px">
                <div style="display:{display}">
                    <span id="c" style="display:contents; color:red; font-size:24px">flex text</span>
                </div>
            </body></html>"#
        ));
        let c = doc.query_selector("#c").unwrap().expect("#c not found");
        assert_run_inherits(
            inline_layout_containing(&doc, "flex text"),
            "flex text",
            c,
            24.0,
        );
    }
}

#[test]
fn contents_text_inherits_letter_spacing() {
    fn width(letter_spacing: &str) -> f32 {
        let doc = layout_doc(&format!(
            r#"<html><body style="margin:0; font-size:24px">
                <div><span style="display:contents; letter-spacing:{letter_spacing}">contents text</span></div>
            </body></html>"#
        ));
        inline_layout_containing(&doc, "contents text")
            .layout
            .full_width()
    }

    let normal = width("0px");
    let spaced = width("4px");
    assert!(
        spaced > normal + 20.0,
        "letter-spacing must apply to text inside display:contents (normal {normal}, spaced {spaced})"
    );
}

#[test]
fn contents_text_restyles_when_inherited_property_changes() {
    let mut doc = layout_doc(
        r#"<html><body style="margin:0; color:black; font-size:16px">
            <div><span id="c" style="display:contents; font-size:16px">contents text</span></div>
        </body></html>"#,
    );
    let c = doc.query_selector("#c").unwrap().expect("#c not found");
    assert_run_inherits(
        inline_layout_containing(&doc, "contents text"),
        "contents text",
        c,
        16.0,
    );

    doc.set_style_property(c, "font-size", "32px");
    doc.resolve(0.0);
    assert_run_inherits(
        inline_layout_containing(&doc, "contents text"),
        "contents text",
        c,
        32.0,
    );
}

#[test]
fn contents_text_does_not_paint_its_background() {
    use anyrender::render_to_buffer;
    use anyrender_vello_cpu::VelloCpuImageRenderer;
    use blitz_paint::paint_scene;

    let mut doc = layout_doc(
        r#"<html><body style="margin:0; background:white">
            <div style="font: 32px sans-serif">
                <span style="display:contents; background:rgb(255,255,0)">contents text</span>
            </div>
        </body></html>"#,
    );
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc.as_mut(), 1.0, 200, 80, 0, 0),
        200,
        80,
    );
    let yellow_pixels = buffer
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[0] > 200 && px[1] > 200 && px[2] < 100)
        .count();
    assert_eq!(
        yellow_pixels, 0,
        "a display:contents element generates no box, so its background must not be painted"
    );
}
