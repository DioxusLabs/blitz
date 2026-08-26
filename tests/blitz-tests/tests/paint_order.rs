//! CSS 2.1 Appendix E: all positioned descendants with z-index: auto share
//! one paint level (step 8) and paint in tree order among themselves.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{DocumentConfig, ScrollBehavior};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_test_harness::Harness;
use blitz_traits::shell::{ColorScheme, Viewport};
use markup5ever::{QualName, local_name, ns};
use std::sync::Arc;

fn pixel_at(html: &str, x: usize, y: usize) -> [u8; 3] {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let idx = (y * 100 + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

fn pixel_after_scroll(html: &str, x: usize, y: usize) -> [u8; 3] {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let scroller = doc.query_selector("#scroller").unwrap().expect("#scroller");
    doc.scroll_by(scroller, 0.0, 75.0, ScrollBehavior::Instant);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let idx = (y * 100 + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

fn center_pixel(html: &str) -> [u8; 3] {
    pixel_at(html, 50, 50)
}

fn harness_center_pixel(harness: &mut Harness) -> [u8; 4] {
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, harness.doc.as_mut(), 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let idx = (50 * 100 + 50) * 4;
    [
        buffer[idx],
        buffer[idx + 1],
        buffer[idx + 2],
        buffer[idx + 3],
    ]
}

#[test]
fn later_relative_sibling_paints_above_earlier_abspos() {
    let px = center_pixel(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:100px; height:100px;">
                <div style="position:absolute; inset:0; background:#0000ff;"></div>
                <div style="position:relative; width:100px; height:100px; background:#ff0000;"></div>
            </div>
        </body></html>"#,
    );
    assert_eq!(
        px,
        [255, 0, 0],
        "later positioned (z-index auto) sibling must paint above earlier abspos sibling"
    );
}

#[test]
fn abspos_paints_above_earlier_static_sibling() {
    let px = center_pixel(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:100px; height:100px;">
                <div style="width:100px; height:100px; background:#0000ff;"></div>
                <div style="position:absolute; inset:0; background:#ff0000;"></div>
            </div>
        </body></html>"#,
    );
    assert_eq!(px, [255, 0, 0], "abspos must paint above in-flow content");
}

#[test]
fn earlier_abspos_stays_below_static_when_later_in_tree_order_is_static() {
    // In-flow content paints below positioned content even when the
    // positioned element comes first in tree order.
    let px = center_pixel(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:100px; height:100px;">
                <div style="position:absolute; inset:0; background:#ff0000;"></div>
                <div style="width:100px; height:100px; background:#0000ff;"></div>
            </div>
        </body></html>"#,
    );
    assert_eq!(
        px,
        [255, 0, 0],
        "positioned content paints above in-flow content regardless of tree order"
    );
}

#[test]
fn nested_stacking_context_is_atomic() {
    let px = center_pixel(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:100px; height:100px;">
                <div style="position:relative; z-index:1; width:100px; height:100px;">
                    <div style="position:absolute; z-index:999; inset:0; background:#ff0000;"></div>
                </div>
                <div style="position:absolute; z-index:2; inset:0; background:#00ff00;"></div>
            </div>
        </body></html>"#,
    );
    assert_eq!(
        px,
        [0, 255, 0],
        "a descendant cannot escape its real stacking context"
    );
}

#[test]
fn z_index_does_not_apply_to_static_non_flex_grid_contexts() {
    let px = center_pixel(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:100px; height:100px;">
                <div style="z-index:2; transform:translateX(0); width:100px; height:100px; background:#ff0000;"></div>
                <div style="position:absolute; z-index:1; inset:0; background:#00ff00;"></div>
            </div>
        </body></html>"#,
    );
    assert_eq!(
        px,
        [0, 255, 0],
        "z-index must not apply to a static non-flex/grid stacking context"
    );
}

#[test]
fn positioned_auto_container_is_not_atomic() {
    let px = center_pixel(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:100px; height:100px;">
                <div style="position:relative; width:100px; height:100px;">
                    <div style="position:absolute; z-index:999; inset:0; background:#ff0000;"></div>
                </div>
                <div style="position:absolute; z-index:2; inset:0; background:#00ff00;"></div>
            </div>
        </body></html>"#,
    );
    assert_eq!(
        px,
        [255, 0, 0],
        "stacked descendants escape a z-index:auto container"
    );
}

#[test]
fn out_of_flow_order_uses_structural_position() {
    let px = center_pixel(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:100px; height:100px;">
                <div>
                    <div style="position:absolute; inset:0; background:#ff0000;"></div>
                </div>
                <div style="position:relative; width:100px; height:100px; background:#0000ff;"></div>
            </div>
        </body></html>"#,
    );
    assert_eq!(
        px,
        [0, 0, 255],
        "containing-block ownership must not replace structural paint order"
    );
}

#[test]
fn out_of_flow_flex_children_ignore_order() {
    let px = center_pixel(
        r#"<html><body style="margin:0">
            <div style="display:flex; position:relative; width:100px; height:100px;">
                <div style="position:absolute; order:999; inset:0; background:#ff0000;"></div>
                <div style="position:absolute; order:-999; inset:0; background:#0000ff;"></div>
            </div>
        </body></html>"#,
    );
    assert_eq!(
        px,
        [0, 0, 255],
        "order must not reorder out-of-flow flex-container children"
    );
}

#[test]
fn stacked_content_keeps_structural_overflow_clips() {
    let html = r#"<html><body style="margin:0; background:#0000ff;">
        <div style="position:relative; width:50px; height:100px; overflow:hidden;">
            <div style="position:absolute; z-index:1; left:0; top:0; width:100px; height:100px; background:#ff0000;"></div>
        </div>
    </body></html>"#;
    assert_eq!(pixel_at(html, 25, 50), [255, 0, 0]);
    assert_eq!(pixel_at(html, 75, 50), [0, 0, 255]);

    let harness = Harness::from_html(
        r#"<html><body style="margin:0;">
            <div id="clip" style="position:relative; width:50px; height:100px; overflow:hidden;">
                <div id="child" style="position:absolute; z-index:1; left:0; top:0; width:100px; height:100px;"></div>
            </div>
        </body></html>"#,
    );
    let child = harness.node("#child");
    assert_eq!(harness.hit_node(25.0, 50.0), child);
    assert_ne!(harness.hit_node(75.0, 50.0), child);
}

#[test]
fn out_of_flow_content_ignores_clips_between_it_and_its_containing_block() {
    let html = r#"<html><body style="margin:0; background:#0000ff;">
        <div style="width:50px; height:100px; overflow:hidden;">
            <div style="position:absolute; z-index:1; left:0; top:0; width:100px; height:100px; background:#ff0000;"></div>
        </div>
    </body></html>"#;
    assert_eq!(pixel_at(html, 75, 50), [255, 0, 0]);
}

#[test]
fn stacked_out_of_flow_content_uses_its_spatial_scroll_ancestry() {
    let html = r#"<html><body style="margin:0; background:#0000ff;">
        <div id="scroller" style="position:relative; width:100px; height:100px; overflow:hidden;">
            <div style="height:200px;"></div>
            <div style="position:absolute; z-index:1; left:0; top:100px; width:100px; height:50px; background:#ff0000;"></div>
        </div>
    </body></html>"#;
    assert_eq!(pixel_after_scroll(html, 50, 50), [255, 0, 0]);
}

#[test]
fn negative_context_paints_above_context_background_but_below_in_flow_content() {
    let px = center_pixel(
        r#"<html><body style="margin:0">
            <div style="position:relative; z-index:0; width:100px; height:100px; background:#0000ff;">
                <div style="position:absolute; z-index:-1; inset:0; background:#ff0000;"></div>
                <div style="width:100px; height:100px; background:#00ff00;"></div>
            </div>
        </body></html>"#,
    );
    assert_eq!(px, [0, 255, 0]);

    let px = center_pixel(
        r#"<html><body style="margin:0">
            <div style="position:relative; z-index:0; width:100px; height:100px; background:#0000ff;">
                <div style="position:absolute; z-index:-1; inset:0; background:#ff0000;"></div>
            </div>
        </body></html>"#,
    );
    assert_eq!(px, [255, 0, 0]);
}

fn style_attr() -> QualName {
    QualName::new(None, ns!(), local_name!("style"))
}

#[test]
fn z_index_restyle_updates_shared_paint_and_hit_order() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:100px; height:100px;">
                <div id="a" style="position:absolute; z-index:1; inset:0;"></div>
                <div id="b" style="position:absolute; z-index:2; inset:0;"></div>
            </div>
        </body></html>"#,
    );
    let a = harness.node("#a");
    let b = harness.node("#b");
    assert_eq!(harness.hit_node(50.0, 50.0), b);

    harness.base_mut().mutate().set_attribute(
        a,
        style_attr(),
        "position:absolute; z-index:3; inset:0;",
    );
    harness.pump();
    assert_eq!(harness.hit_node(50.0, 50.0), a);
}

#[test]
fn stacking_context_boundary_restyle_updates_old_and_new_owners() {
    let mut harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <div style="position:relative; width:100px; height:100px;">
                <div id="wrapper" style="position:relative; width:100px; height:100px;">
                    <div id="child" style="position:absolute; z-index:10; inset:0;"></div>
                </div>
                <div id="sibling" style="position:absolute; z-index:2; inset:0;"></div>
            </div>
        </body></html>"#,
    );
    let wrapper = harness.node("#wrapper");
    let child = harness.node("#child");
    let sibling = harness.node("#sibling");
    assert_eq!(harness.hit_node(50.0, 50.0), child);

    harness.base_mut().mutate().set_attribute(
        wrapper,
        style_attr(),
        "position:relative; width:100px; height:100px; opacity:0.9;",
    );
    harness.pump();
    assert_eq!(harness.hit_node(50.0, 50.0), sibling);

    harness.base_mut().mutate().set_attribute(
        wrapper,
        style_attr(),
        "position:relative; width:100px; height:100px;",
    );
    harness.pump();
    assert_eq!(harness.hit_node(50.0, 50.0), child);
}

#[test]
fn removed_stacking_context_is_not_rebuilt_from_stale_owner() {
    let initial = r#"<html><body style="margin:0">
        <div id="a" style="width:100px; height:100px; opacity:0.5;">
            <div id="b" style="position:relative; width:100px; height:100px; background:rgba(255,0,0,0.5);"></div>
        </div>
    </body></html>"#;
    let final_html = r#"<html><body style="margin:0">
        <div id="a" style="width:100px; height:100px;">
            <div id="b" style="position:relative; width:100px; height:100px; background:rgba(255,0,0,0.5); color:blue;"></div>
        </div>
    </body></html>"#;

    let mut harness = Harness::from_html(initial);
    let a = harness.node("#a");
    let b = harness.node("#b");
    harness
        .base_mut()
        .mutate()
        .set_attribute(a, style_attr(), "width:100px; height:100px;");
    harness.base_mut().mutate().set_attribute(
        b,
        style_attr(),
        "position:relative; width:100px; height:100px; background:rgba(255,0,0,0.5); color:blue;",
    );
    harness.pump();

    let mut fresh = Harness::from_html(final_html);
    assert_eq!(
        harness_center_pixel(&mut harness),
        harness_center_pixel(&mut fresh),
        "incremental paint must match a fresh document after removing a stacking context"
    );
}
