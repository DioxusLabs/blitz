//! `appearance: none` on a checkbox or radio button suppresses the native
//! control, leaving only the element's own CSS box (CSS UI 4 §7.2). Pages use
//! this to hide the native mark and draw their own.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const W: u32 = 40;
const H: u32 = 40;

/// Number of non-white pixels inside the border of a 20x20 checked control at
/// (10, 10) with a 1px black border, styled further by `style`.
fn painted_inside(input_type: &str, style: &str) -> usize {
    let html = format!(
        "<html><body style='margin:0; background:#fff'>\
         <input type={input_type} checked style='display:block; margin:10px; padding:0; \
         width:20px; height:20px; box-sizing:border-box; border:1px solid #000; \
         border-radius:0; background:#fff; {style}'></body></html>"
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(W, H, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, W, H, 0, 0),
        W,
        H,
    );
    let mut count = 0;
    for y in 12..28 {
        for x in 12..28 {
            let i = ((y * W + x) * 4) as usize;
            if buffer[i..i + 3] != [255, 255, 255] {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn a_checkbox_with_appearance_none_paints_no_native_mark() {
    assert_eq!(painted_inside("checkbox", "appearance:none"), 0);
}

#[test]
fn a_radio_with_appearance_none_paints_no_native_mark() {
    assert_eq!(painted_inside("radio", "appearance:none"), 0);
}

#[test]
fn checkboxes_and_radios_keep_their_native_mark_by_default() {
    assert!(painted_inside("checkbox", "") > 0);
    assert!(painted_inside("radio", "") > 0);
    assert!(painted_inside("checkbox", "appearance:auto") > 0);
}
