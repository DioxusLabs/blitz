//! An inset box shadow paints the padding box minus a "hole": the padding box
//! shrunk by the spread and moved by the shadow offset. With no blur the hole
//! has hard edges, so the shadow must be exact geometry: wherever the hole's
//! edge coincides with the padding box's edge nothing may be painted, and the
//! offset, like the spread and blur, is in CSS pixels and must be scaled to
//! device pixels (#1038).

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const WHITE: [u8; 3] = [255, 255, 255];
const RED: [u8; 3] = [255, 0, 0];

/// Renders a single `div` with `style` at the top left of a white page at
/// `scale`, returning device pixel rows.
fn pixels(style: &str, w: u32, h: u32, scale: f32) -> Vec<Vec<[u8; 3]>> {
    let html = format!(
        "<html><body style='margin:0; background:#fff'>\
         <div style='{style}'></div></body></html>"
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(w, h, scale, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let (dw, dh) = ((w as f32 * scale) as usize, (h as f32 * scale) as usize);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, scale as f64, dw as u32, dh as u32, 0, 0),
        dw as u32,
        dh as u32,
    );
    (0..dh)
        .map(|y| {
            (0..dw)
                .map(|x| {
                    let i = (y * dw + x) * 4;
                    [buffer[i], buffer[i + 1], buffer[i + 2]]
                })
                .collect()
        })
        .collect()
}

#[test]
fn offset_is_scaled_to_device_pixels() {
    // 4 CSS px of offset is 8 device px at scale 2.
    let rows = pixels(
        "width:60px; height:60px; background:#fff; box-shadow: inset 4px 0 #f00",
        60,
        60,
        2.0,
    );
    assert_eq!(rows[60][2], RED);
    assert_eq!(rows[60][6], RED, "the offset should cover 8 device px");
    assert_eq!(rows[60][10], WHITE);
}

#[test]
fn zero_blur_shadow_leaves_coincident_edges_clean() {
    // The repro from #1038: a horizontal-only offset leaves the hole's top and
    // bottom edges exactly on the padding box's, where nothing may be painted.
    // At scale 1.5 the 7px margin puts every edge mid-pixel (the box spans device
    // 10.5..370.5 x 10.5..70.5), so the edges are antialiased, and must match the
    // unshadowed box exactly: the top and bottom where the hole's edges coincide
    // with the padding box's, the right where the hole has moved past it.
    let box_style = "margin:7px; width:240px; height:40px; border-radius:7px; \
                     background:#f5f7fa";
    let shadowed = pixels(
        &format!("{box_style}; box-shadow: inset 3px 0 0 #087f86"),
        254,
        54,
        1.5,
    );
    let plain = pixels(box_style, 254, 54, 1.5);

    // Everywhere right of the shadowed left strip (3px = 4.5 device px, plus the
    // rounded corners).
    for (y, (shadowed, plain)) in shadowed.iter().zip(&plain).enumerate() {
        for x in 40..381 {
            assert_eq!(
                shadowed[x], plain[x],
                "the shadow should not show at device ({x}, {y})"
            );
        }
    }
    // The shadow itself, mid-height in the left strip.
    assert_eq!(shadowed[40][12], [0x08, 0x7f, 0x86]);
}

#[test]
fn zero_blur_ring_paints_all_four_edges() {
    let rows = pixels(
        "width:60px; height:60px; background:#fff; box-shadow: inset 0 0 0 2px #f00",
        60,
        60,
        1.0,
    );
    assert_eq!(rows[30][1], RED, "left edge");
    assert_eq!(rows[30][58], RED, "right edge");
    assert_eq!(rows[1][30], RED, "top edge");
    assert_eq!(rows[58][30], RED, "bottom edge");
    assert_eq!(rows[30][3], WHITE, "just inside the ring");
    assert_eq!(rows[30][30], WHITE, "centre");
}

#[test]
fn blurred_inset_shadow_still_paints() {
    let rows = pixels(
        "width:60px; height:60px; background:#fff; box-shadow: inset 0 0 8px #000",
        60,
        60,
        1.0,
    );
    assert_ne!(rows[30][2], WHITE, "the blur should darken near the edge");
    let centre = rows[30][30];
    assert!(
        centre.iter().all(|&c| c >= 250),
        "the centre should stay (near) white, got {centre:?}"
    );
}
