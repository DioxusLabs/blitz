//! A non-atomic inline box paints no layer of its own, so its `opacity` has to
//! reach the glyphs, decorations and background that it paints in the inline
//! formatting context of its block.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const W: usize = 200;
const H: usize = 60;

/// The red pixels of a block whose inline content is `inline`. The text is red,
/// and each `u` span draws a red underline.
fn red_pixels(inline: &str) -> Vec<[u8; 3]> {
    let html = format!(
        "<html><body style='margin:0; background:#fff'>\
         <style>u {{ text-decoration: underline #f00 }}</style>\
         <div style='font:40px/1 sans-serif; padding:10px; color:#f00'>{inline}</div>\
         </body></html>"
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
    buffer
        .chunks_exact(4)
        .map(|px| [px[0], px[1], px[2]])
        .filter(|px| px[0] > 200 && px[1] < 230 && px[1] == px[2])
        .collect()
}

#[test]
fn an_inline_box_with_zero_opacity_paints_nothing() {
    assert!(!red_pixels("<u>XXXX</u>").is_empty());

    let painted = red_pixels("<u style='opacity:0; background:#f00'>XXXX</u>");
    assert!(
        painted.is_empty(),
        "an opacity: 0 span painted {} red pixels",
        painted.len()
    );
}

#[test]
fn opacity_of_nested_inline_boxes_multiplies() {
    // 0.5 inside 0.5 is 0.25: red over white is at most about rgb(255, 191, 191).
    let pixels =
        red_pixels("<span style='opacity:0.5'><span style='opacity:0.5'>XXXX</span></span>");
    assert!(!pixels.is_empty());
    let darkest = pixels.iter().map(|px| px[1]).min().unwrap();
    assert!(
        darkest >= 180,
        "nested spans at opacity 0.5 painted a pixel at green {darkest}, expected about 191"
    );
}
