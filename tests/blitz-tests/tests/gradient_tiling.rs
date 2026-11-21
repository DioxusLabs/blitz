//! Tiling of gradient background layers (CSS Backgrounds 3 §2.4
//! `background-repeat`).
//!
//! A non-repeating linear gradient whose direction is parallel to a tile axis
//! is painted with a single fill whose brush repeats along the gradient line,
//! so any number of tiles is painted (previously more than 500 tiles painted
//! nothing). Other gradients are still painted tile by tile.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const SIZE: usize = 100;

/// Renders a 100x100 div with the given background shorthand on a 100x100
/// viewport (on a solid green page background) and returns the RGBA buffer.
fn render(background: &str) -> Vec<u8> {
    let html = format!(
        r#"<html><body style="margin:0; background:#00ff00;">
            <div style="width:100px; height:100px; background: {background};"></div>
        </body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(
                SIZE as u32,
                SIZE as u32,
                1.0,
                ColorScheme::Light,
            )),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc.as_mut(), 1.0, SIZE as u32, SIZE as u32, 0, 0),
        SIZE as u32,
        SIZE as u32,
    )
}

fn px(buffer: &[u8], x: usize, y: usize) -> [u8; 3] {
    let idx = (y * SIZE + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

const BLACK: [u8; 3] = [0, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];
const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];

#[test]
fn many_small_tiles_are_painted() {
    // 50x50 = 2500 tiles of 2x2px: each tile is one black and one white column
    let buffer = render("linear-gradient(90deg,#000 50%,#fff 50%) 0 0 / 2px 2px");
    for y in [0, 50, 99] {
        for x in 0..SIZE {
            let expected = if x % 2 == 0 { BLACK } else { WHITE };
            assert_eq!(px(&buffer, x, y), expected, "pixel ({x}, {y})");
        }
    }
}

#[test]
fn horizontal_gradient_tiles_match_tile_phase() {
    // Tiles start at x = 3 + 10k: red over the first half, blue over the second.
    // The partial tile left of x = 3 is the second half of a tile, i.e. blue.
    let buffer = render("linear-gradient(90deg, #f00 50%, #00f 50%) 3px 0 / 10px 10px");
    for y in [0, 55] {
        for x in 0..SIZE {
            let expected = if (x + 7) % 10 < 5 { RED } else { BLUE };
            assert_eq!(px(&buffer, x, y), expected, "pixel ({x}, {y})");
        }
    }
}

#[test]
fn vertical_gradient_tiles_restart_each_tile() {
    let buffer = render("linear-gradient(#f00, #00f) 0 0 / 10px 10px");
    for x in [0, 42, 99] {
        for tile in 0..10 {
            let top = px(&buffer, x, tile * 10);
            let bottom = px(&buffer, x, tile * 10 + 9);
            assert!(
                top[0] > 200 && top[2] < 55,
                "row {} must be red-ish, got {top:?}",
                tile * 10
            );
            assert!(
                bottom[2] > 200 && bottom[0] < 55,
                "row {} must be blue-ish, got {bottom:?}",
                tile * 10 + 9
            );
        }
    }
}

#[test]
fn diagonal_gradient_still_uses_tiled_fallback() {
    // 25 tiles of 20x20px, painted tile by tile
    let buffer = render("linear-gradient(45deg,#000 50%,#fff 50%) 0 0 / 20px 20px");
    let (mut black, mut white) = (0, 0);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = px(&buffer, x, y);
            black += (p == BLACK) as u32;
            white += (p == WHITE) as u32;
            if x + 20 < SIZE {
                assert_eq!(p, px(&buffer, x + 20, y), "pixel ({x}, {y}) vs +20px");
            }
            if y + 20 < SIZE {
                assert_eq!(p, px(&buffer, x, y + 20), "pixel ({x}, {y}) vs +20px");
            }
        }
    }
    assert!(black > 1000, "expected black pixels, got {black}");
    assert!(white > 1000, "expected white pixels, got {white}");
}

#[test]
fn repeating_gradient_tiles_still_paint() {
    // 2px black / 2px white stripes; 40px tiles hold exactly 10 periods.
    // (`to right` rather than `90deg`: the tiled fallback centres angle-form
    // gradient lines on the border box rather than the tile.)
    let buffer =
        render("repeating-linear-gradient(to right,#000 0 2px,#fff 2px 4px) 0 0 / 40px 40px");
    for y in [0, 50] {
        for x in 0..SIZE {
            let expected = if x % 4 < 2 { BLACK } else { WHITE };
            assert_eq!(px(&buffer, x, y), expected, "pixel ({x}, {y})");
        }
    }
}
