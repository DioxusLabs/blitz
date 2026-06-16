//! background-size: cover/contain scaling.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_dom::node::{ImageData, RasterImageData};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

/// Renders a 100x100 div with the given background shorthand, injecting a
/// 200x100 solid-red image as the loaded background, and returns the pixel
/// at (x, y). The div sits on a solid blue page background.
fn pixel(background: &str, x: usize, y: usize) -> [u8; 3] {
    let html = format!(
        r#"<html><body style="margin:0; background:#0000ff;">
            <div id="box" style="width:100px; height:100px; background: {background};"></div>
        </body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let box_id = doc.query_selector("#box").unwrap().expect("#box");
    {
        // 200x100 solid red RGBA
        let data: Vec<u8> = std::iter::repeat([255u8, 0, 0, 255])
            .take(200 * 100)
            .flatten()
            .collect();
        let node = doc.get_node_mut(box_id).unwrap();
        let el = node.element_data_mut().unwrap();
        for layer in el.background_images.iter_mut().flatten() {
            layer.status = blitz_dom::node::Status::Ok;
            layer.image = ImageData::Raster(RasterImageData::new(200, 100, Arc::new(data.clone())));
        }
    }
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc.as_mut(), 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let idx = (y * 100 + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

const RED: [u8; 3] = [255, 0, 0];

#[test]
fn background_cover_fills_the_box() {
    // 200x100 image covering a 100x100 box: scaled to 200x100*1.0 -> wait,
    // cover ratio = max(100/200, 100/100) = 1.0 -> 200x100 crop. Every
    // pixel of the box must be image (red), including near the bottom.
    let px = pixel(
        "url('https://example.com/x.png') center/cover no-repeat",
        50,
        95,
    );
    assert_eq!(px, RED, "cover must fill the box vertically");
}

#[test]
fn background_contain_letterboxes() {
    // contain ratio = min(0.5, 1.0) = 0.5 -> 100x50 centered: the bottom
    // strip shows the element/page background, not the image.
    let px = pixel(
        "url('https://example.com/x.png') center/contain no-repeat",
        50,
        95,
    );
    assert_ne!(px, RED, "contain must letterbox, not fill");
}
