//! A clip-path clips the element's decorations as well as its content.
use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn pixels(css: &str, scale: f32) -> (usize, Vec<u8>) {
    let html = format!(
        "<html><body style='margin:0;background:white'><div style='position:absolute;\
         left:40px;top:40px;width:40px;height:40px;background:red;{css}'></div></body></html>"
    );
    let size = (120.0 * scale) as u32;
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(size, size, scale, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let pixels = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc.as_mut(), scale as f64, size, size, 0, 0),
        size,
        size,
    );
    (size as usize, pixels)
}

fn at(buffer: &(usize, Vec<u8>), scale: f32, x: f32, y: f32) -> &[u8] {
    let index = ((y * scale) as usize * buffer.0 + (x * scale) as usize) * 4;
    &buffer.1[index..index + 3]
}

#[test]
fn clip_path_clips_an_outer_spread_shadow() {
    for scale in [0.5, 1.0, 2.0] {
        let buffer = pixels(
            "box-shadow:0 0 0 100vmax blue;clip-path:inset(0 -100vmax)",
            scale,
        );
        assert_eq!(at(&buffer, scale, 60.0, 20.0), [255; 3]);
        assert_eq!(at(&buffer, scale, 60.0, 100.0), [255; 3]);
        assert_eq!(at(&buffer, scale, 10.0, 60.0), [0, 0, 255]);
        assert_eq!(at(&buffer, scale, 110.0, 60.0), [0, 0, 255]);
        assert_eq!(at(&buffer, scale, 60.0, 60.0), [255, 0, 0]);
    }
}

#[test]
fn clip_path_clips_the_outline() {
    for scale in [0.5, 1.0, 2.0] {
        let buffer = pixels("outline:20px solid blue;clip-path:inset(0)", scale);
        for (x, y) in [(30.0, 60.0), (90.0, 60.0), (60.0, 30.0), (60.0, 90.0)] {
            assert_eq!(at(&buffer, scale, x, y), [255; 3]);
        }
        assert_eq!(at(&buffer, scale, 60.0, 60.0), [255, 0, 0]);
    }
}

#[test]
fn an_expanded_clip_keeps_the_allowed_part_of_the_outline() {
    let buffer = pixels("outline:20px solid blue;clip-path:inset(-10px)", 1.0);
    assert_eq!(at(&buffer, 1.0, 25.0, 60.0), [255; 3]);
    assert_eq!(at(&buffer, 1.0, 35.0, 60.0), [0, 0, 255]);
    let unclipped = pixels("outline:20px solid blue", 1.0);
    assert_eq!(at(&unclipped, 1.0, 25.0, 60.0), [0, 0, 255]);
}
