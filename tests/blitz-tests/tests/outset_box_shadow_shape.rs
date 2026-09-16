//! An outset box shadow has the shape of the element it falls from, corner
//! for corner. A shadow with no blur and no spread therefore hides behind the
//! element exactly, which is what an elevation of 0 relies on, and an offset
//! one peeks out along the rounded edge it came from.
//!
//! The expected pixels come from Chromium rendering the same boxes.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const W: usize = 120;
const H: usize = 80;

/// A 60x34 box at (20, 20), rounded on its start side like the end segment of
/// a segmented button, styled further by `style`.
fn pixels(style: &str) -> Vec<Vec<[u8; 3]>> {
    let html = format!(
        "<html><body style='margin:0; background:#fff'>\
         <div style='width:60px; height:34px; box-sizing:border-box; background:#fff; \
         border:2px solid #808080; margin:20px; {style}'></div></body></html>"
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(W as u32, H as u32, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, W as u32, H as u32, 0, 0),
        W as u32,
        H as u32,
    );
    (0..H)
        .map(|y| {
            (0..W)
                .map(|x| {
                    let i = (y * W + x) * 4;
                    [buffer[i], buffer[i + 1], buffer[i + 2]]
                })
                .collect()
        })
        .collect()
}

fn is_red(px: [u8; 3]) -> bool {
    px[0] > 150 && px[1] < 120
}

/// The leftmost red pixel in a row, if any.
fn red_from(rows: &[Vec<[u8; 3]>], y: usize) -> Option<usize> {
    rows[y].iter().position(|&px| is_red(px))
}

#[test]
fn a_shadow_with_no_blur_or_spread_hides_behind_the_element() {
    let rows = pixels("border-radius: 17px 0 0 17px; box-shadow: #d00 0 0 0");

    let leaked: Vec<(usize, usize)> = (0..H)
        .filter_map(|y| red_from(&rows, y).map(|x| (x, y)))
        .collect();
    assert!(
        leaked.is_empty(),
        "the shadow should be invisible, but showed at {leaked:?}"
    );
}

#[test]
fn an_offset_shadow_follows_the_rounded_edge() {
    let rows = pixels("border-radius: 17px 0 0 17px; box-shadow: #d00 -6px 0 0");

    // The rounded start pulls the shadow in towards the middle of the box,
    // where it reaches the full 6px of offset (box left edge 20, mid-height
    // 37). Chromium's arc, to a pixel of antialiasing at the fractional rows.
    for (y, expected) in [(20, 27), (22, 22), (26, 18), (30, 15), (37, 14)] {
        let got = red_from(&rows, y).unwrap_or_else(|| panic!("no shadow in row {y}"));
        assert!(
            got.abs_diff(expected) <= 1,
            "row {y}: shadow starts at {got}, expected about {expected}"
        );
    }
    // and nothing on the square end
    assert!(rows[37][82..].iter().all(|&px| !is_red(px)));
}

#[test]
fn a_blurred_shadow_still_paints_around_the_element() {
    let rows = pixels("border-radius: 8px; box-shadow: #d00 0 0 6px");

    let outside = rows[37][14];
    assert_ne!(
        outside,
        [255, 255, 255],
        "a blurred shadow reaches past the border box"
    );
}
