//! A window with safe-area insets paints the document inside the insets, but the
//! canvas background should still cover the inset areas (e.g. behind the status
//! bar on a phone) instead of leaving the renderer's clear color there.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene_with_safe_area;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const SURFACE: u32 = 100;
// Insets: top 20, left 10, bottom 30, right 0.
const VIEWPORT: [u32; 2] = [90, 50];
const ORIGIN: [u32; 2] = [10, 20];

fn render(html: &str) -> Vec<u8> {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(
                VIEWPORT[0],
                VIEWPORT[1],
                1.0,
                ColorScheme::Light,
            )),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            paint_scene_with_safe_area(scene, &mut doc, 1.0, VIEWPORT, ORIGIN, [SURFACE, SURFACE])
        },
        SURFACE,
        SURFACE,
    )
}

fn pixel(buffer: &[u8], x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * SURFACE + x) * 4) as usize;
    [
        buffer[idx],
        buffer[idx + 1],
        buffer[idx + 2],
        buffer[idx + 3],
    ]
}

#[test]
fn canvas_background_fills_safe_area_insets() {
    let buffer = render(
        r#"<html style="background:#00ff00"><body style="margin:0">
            <div style="width:20px; height:20px; background:#ff0000"></div>
        </body></html>"#,
    );
    let green = [0, 255, 0, 255];
    assert_eq!(pixel(&buffer, 50, 5), green, "top inset");
    assert_eq!(pixel(&buffer, 5, 50), green, "left inset");
    assert_eq!(pixel(&buffer, 50, 90), green, "bottom inset");
    // Content is still placed at the viewport origin.
    assert_eq!(pixel(&buffer, 15, 25), [255, 0, 0, 255], "content");
    assert_eq!(
        pixel(&buffer, 5, 25),
        green,
        "content stays inside the viewport"
    );
}
