//! Regression tests for DioxusLabs/blitz#837: `1px` borders that vanished.
//!
//! `border: 1px solid red; border-top-width: 0; border-radius: 8px` must draw
//! the left, right and bottom sides. Two separate bugs each erased them:
//!
//! - the corner arc split angle was computed from the ratio of the two
//!   adjacent border widths, so a zero-width side produced `NaN` and, since
//!   all four edges are filled as one path, the whole outline was dropped;
//! - at a fractional device pixel ratio a one-device-pixel border is less than
//!   a CSS pixel wide, and rounding its two edges to whole CSS pixels could
//!   land both on the same value, leaving a zero-width border to paint.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_test_harness::{Harness, HarnessOptions};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const CSS_W: u32 = 300;
const CSS_H: u32 = 120;

/// Render a 260x80 panel at (20, 20) styled by `style`, at `scale`, and
/// count the red pixels in each edge band of the panel.
///
/// Returns `[top, right, bottom, left]`.
fn red_edge_pixels(style: &str, scale: f64) -> [usize; 4] {
    let html = format!(
        "<html><body style='margin:0; background:#fff'>\
         <div style='margin:20px; height:80px; box-sizing:border-box; {style}'></div>\
         </body></html>"
    );
    let w = (CSS_W as f64 * scale) as u32;
    let h = (CSS_H as f64 * scale) as u32;
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(w, h, scale as f32, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, scale, w, h, 0, 0),
        w,
        h,
    );

    let (w, h) = (w as usize, h as usize);
    let mut counts = [0; 4];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let is_red = buffer[i] > 150 && buffer[i + 1] < 120 && buffer[i + 2] < 120;
            if !is_red {
                continue;
            }
            // Away from the corners, classify by the nearest edge of the panel.
            let (fx, fy) = (x as f64 / w as f64, y as f64 / h as f64);
            if (0.2..0.8).contains(&fx) {
                counts[if fy < 0.5 { 0 } else { 2 }] += 1;
            } else if (0.3..0.7).contains(&fy) {
                counts[if fx < 0.5 { 3 } else { 1 }] += 1;
            }
        }
    }
    counts
}

const ISSUE_STYLE: &str = "border: 1px solid #ff0000; border-top-width: 0; border-radius: 8px; \
                           background: #212121; overflow: hidden;";

#[test]
fn a_rounded_border_with_one_zero_width_side_draws_the_other_three() {
    let [top, right, bottom, left] = red_edge_pixels(ISSUE_STYLE, 1.0);
    assert_eq!(top, 0, "the top side has zero width and must not be drawn");
    assert!(right > 0, "the right side vanished");
    assert!(bottom > 0, "the bottom side vanished");
    assert!(left > 0, "the left side vanished");
}

#[test]
fn one_device_pixel_borders_survive_fractional_scaling() {
    for scale in [1.25, 1.5, 1.75] {
        for style in [
            ISSUE_STYLE,
            "border: 1px solid #ff0000; border-top-width: 0;",
            "border: 1px solid #ff0000;",
        ] {
            let [top, right, bottom, left] = red_edge_pixels(style, scale);
            let expect_top = !style.contains("border-top-width: 0");
            assert_eq!(
                top > 0,
                expect_top,
                "top side wrong at scale {scale} for `{style}`"
            );
            assert!(
                right > 0 && bottom > 0 && left > 0,
                "sides vanished at scale {scale} for `{style}`: \
                 right={right} bottom={bottom} left={left}"
            );
        }
    }
}

/// The layout a `1px` border is painted from must stay a whole, non-zero
/// number of device pixels wide after rounding, whatever the device pixel
/// ratio.
#[test]
fn rounded_layout_keeps_one_device_pixel_borders() {
    for scale in [1.0f32, 1.25, 1.5, 1.75, 2.0] {
        let harness = Harness::from_html_with(
            "<div id=p style='margin:20px; padding:20px; \
             border:1px solid red; border-top-width:0'>panel</div>",
            HarnessOptions {
                scale,
                ..Default::default()
            },
        );
        let id = harness.node("#p");
        let base = harness.base();
        let border = base.get_node(id).unwrap().final_layout().border;
        for (side, width) in [
            ("left", border.left),
            ("right", border.right),
            ("bottom", border.bottom),
        ] {
            let device_px = width * scale;
            assert!(
                device_px >= 1.0 - 1e-3 && (device_px - device_px.round()).abs() < 1e-3,
                "{side} border is {width} CSS px ({device_px} device px) at scale {scale}"
            );
        }
        assert_eq!(border.top, 0.0, "top border at scale {scale}");
    }
}
