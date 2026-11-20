//! Root and `<body>` backgrounds propagate to the canvas
//! (<https://drafts.csswg.org/css-backgrounds/#special-backgrounds>).

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_dom::node::{ImageData, RasterImageData};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const SIZE: usize = 100;
const RED: [u8; 3] = [255, 0, 0];
const GREY: [u8; 3] = [0x22, 0x22, 0x22];

/// Renders `html` into a 100x100 canvas, with every background image of the
/// element matching `selector` loaded as a 20x20 solid red image.
fn render(html: &str, selector: &str) -> Vec<u8> {
    let mut doc = HtmlDocument::from_html(
        html,
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
    let id = doc.query_selector(selector).unwrap().expect(selector);
    {
        let data: Vec<u8> = std::iter::repeat_n([255u8, 0, 0, 255], 20 * 20)
            .flatten()
            .collect();
        let el = doc.get_node_mut(id).unwrap().element_data_mut().unwrap();
        for layer in el.background_images.iter_mut().flatten() {
            layer.status = blitz_dom::node::Status::Ok;
            layer.image = ImageData::Raster(RasterImageData::new(20, 20, Arc::new(data.clone())));
        }
    }
    doc.resolve(0.0);
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc.as_mut(), 1.0, SIZE as u32, SIZE as u32, 0, 0),
        SIZE as u32,
        SIZE as u32,
    )
}

fn pixel(buffer: &[u8], x: usize, y: usize) -> [u8; 3] {
    let idx = (y * SIZE + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

#[test]
fn body_background_image_propagates_to_canvas() {
    // The empty body is zero height, so the image is only visible if it is
    // painted on the canvas, at the top-left of the root element's box.
    let buffer = render(
        r#"<html><body style="margin: 0; background: #222 url('https://example.com/logo.png') no-repeat"></body></html>"#,
        "body",
    );
    assert_eq!(pixel(&buffer, 5, 5), RED);
    assert_eq!(pixel(&buffer, 15, 15), RED);
    assert_eq!(pixel(&buffer, 25, 5), GREY);
    assert_eq!(pixel(&buffer, 50, 50), GREY);
}

#[test]
fn root_background_image_repeats_over_canvas() {
    // A repeated root background tiles the whole canvas, not just the
    // (20px tall) root element box.
    let buffer = render(
        r#"<html style="height: 20px; background: #222 url('https://example.com/logo.png') repeat-y"><body style="margin: 0"></body></html>"#,
        "html",
    );
    assert_eq!(pixel(&buffer, 5, 5), RED);
    assert_eq!(pixel(&buffer, 5, 90), RED);
    assert_eq!(pixel(&buffer, 50, 90), GREY);
}

#[test]
fn propagated_body_background_is_not_painted_in_body_box() {
    // Once the body background has moved to the canvas, the body box itself
    // paints nothing, so its own top-left corner shows the canvas color
    // rather than a second copy of the image.
    let buffer = render(
        r#"<html><body style="margin: 30px; height: 40px; background: #222 url('https://example.com/logo.png') no-repeat 0 0"></body></html>"#,
        "body",
    );
    // Positioned relative to the root element (0,0), not the body (30,30)
    assert_eq!(pixel(&buffer, 5, 5), RED);
    assert_eq!(pixel(&buffer, 35, 35), GREY);
}
