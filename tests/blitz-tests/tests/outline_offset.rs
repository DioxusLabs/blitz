//! `outline-offset` moves the outline away from the border box, or into it
//! when negative.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const RED: [u8; 3] = [255, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];

/// A 100x60 box at (20, 20) with a 2px red outline at `offset`, optionally
/// inside a parent of the same size that clips.
fn page(offset: &str, clipped: bool) -> String {
    let outlined = format!(
        "position: absolute; left: 0; top: 0; width: 100px; height: 60px; \
         outline: 2px solid #ff0000; outline-offset: {offset}"
    );
    let inner = format!("<div style='{outlined}'></div>");
    let parent_overflow = if clipped { "overflow: hidden" } else { "" };
    format!(
        "<html><head><style>body {{ margin: 0; background: #fff }}</style></head><body>\
         <div style='position: absolute; left: 20px; top: 20px; width: 100px; height: 60px; \
                     {parent_overflow}'>{inner}</div></body></html>"
    )
}

/// The colour at (x, 50) of a 160x100 render of `html`, halfway down the box's
/// left edge.
fn pixel_at(html: &str, x: u32) -> [u8; 3] {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(160, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, 160, 100, 0, 0),
        160,
        100,
    );
    let idx = ((50 * 160 + x) * 4) as usize;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

/// Without an offset the outline hugs the border box from outside: x 18..20.
#[test]
fn no_offset_draws_the_outline_around_the_border_box() {
    let html = page("0px", false);
    assert_eq!(pixel_at(&html, 19), RED);
    assert_eq!(pixel_at(&html, 21), WHITE);
}

/// A positive offset leaves a gap: the outline is at x 14..16.
#[test]
fn a_positive_offset_moves_the_outline_out() {
    let html = page("4px", false);
    assert_eq!(pixel_at(&html, 15), RED);
    assert_eq!(pixel_at(&html, 19), WHITE);
}

/// A negative offset draws it inside the box: x 24..26.
#[test]
fn a_negative_offset_moves_the_outline_in() {
    let html = page("-6px", false);
    assert_eq!(pixel_at(&html, 25), RED);
    assert_eq!(pixel_at(&html, 19), WHITE);
}

/// An outline set into the box survives a parent that clips at the box's
/// edge, which an outline drawn around the border box would not.
#[test]
fn an_inset_outline_shows_inside_a_clipping_parent() {
    let html = page("-6px", true);
    assert_eq!(pixel_at(&html, 25), RED);
}
