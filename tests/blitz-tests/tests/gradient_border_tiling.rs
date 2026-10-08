//! Integration tests for gradient background sizing, border tiling, and canonical geometry.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn render_html(html: &str, width: u32, height: u32) -> Vec<u8> {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(width, height, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc.as_mut(), 1.0, width, height, 0, 0),
        width,
        height,
    )
}

fn pixel_at(buffer: &[u8], width: u32, x: usize, y: usize) -> [u8; 3] {
    let idx = (y * width as usize + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const BLACK: [u8; 3] = [0, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];

/// Case 1: oversized_gradient_200_percent_not_shrunken
///
/// background-size: 200% 100% on a 100x100 box yields a 200px wide tile.
/// With linear-gradient(to right, #ff0000 50%, #00ff00 50%), the red portion
/// covers the first 50% (0..100px), which corresponds to the entire 100px width
/// of the visible element. All pixels inside the box must be pure red (#ff0000),
/// not green (#00ff00).
#[test]
fn oversized_gradient_200_percent_not_shrunken() {
    let html = r#"<html><body style="margin:0; background:#0000ff;">
        <div id="box" style="width:100px; height:100px; background-size: 200% 100%; background-image: linear-gradient(to right, #ff0000 50%, #00ff00 50%); background-repeat: no-repeat;"></div>
    </body></html>"#;

    let buf = render_html(html, 200, 200);

    // Test pixels inside the 100x100 box
    for x in [20, 50, 80, 95] {
        let px = pixel_at(&buf, 200, x, 50);
        assert_eq!(
            px, RED,
            "Pixel at ({x}, 50) must be RED [255, 0, 0], got {px:?}"
        );
        assert_ne!(
            px, GREEN,
            "Pixel at ({x}, 50) must NOT be GREEN [0, 255, 0]"
        );
    }
}

/// Case 2: gradients_with_border_preserves_canonical_dimensions
///
/// Reproduce exact WPT gradients-with-border.html scenario:
/// A 200x100 div with a 100px left border and 10px top/bottom/right borders.
/// Background origin defaults to padding-box (200x100), positioned at (100, 10).
/// The linear-gradient(to right top, #000000 49%, #ffffff 50%) must sample
/// according to the canonical 200x100 padding-box geometry, preserving the
/// diagonal angle and color transition.
#[test]
fn gradients_with_border_preserves_canonical_dimensions() {
    let html = r#"<html><body style="margin:0; background:#ffffff;">
        <div id="bordered" style="width: 200px; height: 100px; border: solid 10px blue; border-left-width: 100px; background-image: linear-gradient(to right top, #000000 49%, #ffffff 50%);"></div>
    </body></html>"#;

    let buf = render_html(html, 350, 150);

    // Left border (x in 0..100, y in 0..120) must be solid blue
    let border_px = pixel_at(&buf, 350, 50, 60);
    assert_eq!(border_px, BLUE, "Left border must be blue");

    // Inside the content/padding area:
    // Padding box begins at x = 100, y = 10 and has size 200x100.
    // Near bottom-left of content (local x=20, local y=80 -> doc x=120, doc y=90):
    // Gradient to right top starts black at bottom-left and transitions to white at top-right.
    let bl_px = pixel_at(&buf, 350, 120, 90);
    assert_eq!(
        bl_px, BLACK,
        "Bottom-left content area must be black, got {bl_px:?}"
    );

    // Near top-right of content (local x=180, local y=20 -> doc x=280, doc y=30):
    let tr_px = pixel_at(&buf, 350, 280, 30);
    assert_eq!(
        tr_px, WHITE,
        "Top-right content area must be white, got {tr_px:?}"
    );

    // At local (20, 20) -> doc (120, 30), t = (2*20 - 20 + 100)/500 = 0.24 (< 49% -> black)
    let tl_px = pixel_at(&buf, 350, 120, 30);
    assert_eq!(
        tl_px, BLACK,
        "Top-left content area at (120, 30) must be black, got {tl_px:?}"
    );
}

/// Case 3: transparent_border_repeat_retains_tiling
///
/// A 100x100 div with 10px semi-transparent white border and repeating red gradient.
/// With background-clip: border-box (default), background tiles must extend into
/// the border area under the semi-transparent border, producing blended reddish pixels
/// instead of revealing the black page background.
#[test]
fn transparent_border_repeat_retains_tiling() {
    let html = r#"<html><body style="margin:0; background:#000000;">
        <div id="box" style="width: 100px; height: 100px; border: 10px solid rgba(255, 255, 255, 0.5); background-image: linear-gradient(to right, #ff0000, #ff0000); background-repeat: repeat;"></div>
    </body></html>"#;

    let buf = render_html(html, 150, 150);

    // Content area inside border (e.g. x=50, y=50) must be pure red [255, 0, 0]
    let content_px = pixel_at(&buf, 150, 50, 50);
    assert_eq!(
        content_px, RED,
        "Content area must be pure red, got {content_px:?}"
    );

    // In the border area (e.g. x=5, y=50 on the left border):
    // 50% white blended over red [255, 0, 0] yields [255, ~128, ~128].
    // If background did not tile into border, 50% white over black [0, 0, 0]
    // would yield [~128, ~128, ~128] with red channel ~128.
    let border_px = pixel_at(&buf, 150, 5, 50);
    assert_eq!(
        border_px[0], 255,
        "Border red channel must be 255 from underlying red gradient tile, got {border_px:?}"
    );
    assert!(
        border_px[1] > 100 && border_px[1] < 155,
        "Border green channel must be blended (~128), got {border_px:?}"
    );
    assert!(
        border_px[2] > 100 && border_px[2] < 155,
        "Border blue channel must be blended (~128), got {border_px:?}"
    );
}
