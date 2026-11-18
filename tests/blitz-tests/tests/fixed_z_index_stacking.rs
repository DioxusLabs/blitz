//! Stacking follows the tree of stacking contexts, not the containing block:
//! a z-indexed `position: fixed` box whose containing block is the viewport
//! still stacks within the stacking context of its nearest stacking-context
//! ancestor (CSS 2.1 Appendix E). Issue #1401.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_test_harness::Harness;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const CSS: &str = "body { margin: 0 }
    .scrim { position: fixed; z-index: 29; left: 0; top: 0; width: 100vw; height: 100vh; background: #0000ff }
    .menu { position: absolute; z-index: 31; top: 36px; left: 20px; width: 120px; height: 80px; background: #ff0000 }";

fn page(ctx_style: &str) -> String {
    format!(
        "<html><head><style>{CSS}</style></head><body>\
         <div style='{ctx_style}'>\
         <div class=scrim id=scrim></div><div class=menu id=menu></div>\
         </div></body></html>"
    )
}

fn id_of(harness: &Harness, x: f32, y: f32) -> String {
    let node = harness.hit_node(x, y);
    harness
        .base()
        .get_node(node)
        .and_then(|n| n.attr(markup5ever::local_name!("id")).map(str::to_string))
        .unwrap_or_default()
}

fn hit_id(ctx_style: &str) -> String {
    // (80, 76) is inside .menu (x 20..140, y 36..116).
    id_of(&Harness::from_html(&page(ctx_style)), 80.0, 76.0)
}

#[test]
fn higher_z_abspos_wins_over_fixed_without_stacking_ancestor() {
    assert_eq!(hit_id(""), "menu");
}

#[test]
fn higher_z_abspos_wins_over_fixed_in_z_indexed_relative_ancestor() {
    assert_eq!(hit_id("position: relative; z-index: 2"), "menu");
}

#[test]
fn higher_z_abspos_wins_over_fixed_in_opacity_ancestor() {
    assert_eq!(hit_id("opacity: 0.99"), "menu");
}

fn pixel(html: &str, x: usize, y: usize) -> [u8; 3] {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(200, 200, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, 200, 200, 0, 0),
        200,
        200,
    );
    let idx = (y * 200 + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

#[test]
fn higher_z_abspos_paints_over_fixed_in_z_indexed_relative_ancestor() {
    let html = page("position: relative; z-index: 2");
    assert_eq!(pixel(&html, 80, 76), [255, 0, 0], "menu");
    assert_eq!(pixel(&html, 180, 180), [0, 0, 255], "scrim");
}

#[test]
fn fixed_box_in_opacity_ancestor_is_painted() {
    let html = page("opacity: 0.99");
    assert_eq!(pixel(&html, 180, 180), [0, 0, 252], "scrim");
}
