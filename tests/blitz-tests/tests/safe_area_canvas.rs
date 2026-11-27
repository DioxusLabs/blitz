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

fn render(html: &str, canvas: [u32; 4]) -> Vec<u8> {
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
        |scene| paint_scene_with_safe_area(scene, &mut doc, 1.0, VIEWPORT, ORIGIN, canvas),
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

const HTML: &str = r#"<html style="background:#00ff00"><body style="margin:0">
    <div style="width:20px; height:20px; background:#ff0000"></div>
</body></html>"#;

const GREEN: [u8; 4] = [0, 255, 0, 255];

#[test]
fn canvas_background_fills_safe_area_insets() {
    let buffer = render(HTML, [0, 0, SURFACE, SURFACE]);
    assert_eq!(pixel(&buffer, 50, 5), GREEN, "top inset");
    assert_eq!(pixel(&buffer, 5, 50), GREEN, "left inset");
    assert_eq!(pixel(&buffer, 50, 90), GREEN, "bottom inset");
    // Content is still placed at the viewport origin.
    assert_eq!(pixel(&buffer, 15, 25), [255, 0, 0, 255], "content");
    assert_eq!(
        pixel(&buffer, 5, 25),
        GREEN,
        "content stays inside the viewport"
    );
}

#[test]
fn canvas_background_only_fills_the_requested_insets() {
    // Only the top inset, as on Android, where the other insets hold the navigation buttons.
    let buffer = render(
        HTML,
        [
            ORIGIN[0],
            0,
            ORIGIN[0] + VIEWPORT[0],
            ORIGIN[1] + VIEWPORT[1],
        ],
    );
    assert_eq!(pixel(&buffer, 50, 5), GREEN, "top inset");
    assert_ne!(pixel(&buffer, 5, 50), GREEN, "left inset");
    assert_ne!(pixel(&buffer, 50, 90), GREEN, "bottom inset");
}
