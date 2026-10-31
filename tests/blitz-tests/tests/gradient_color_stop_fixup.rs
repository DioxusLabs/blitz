//! A color stop whose position is less than a position before it is raised
//! to the largest earlier position (CSS Images 3 §3.4.3 "Color Stop Fixup").
//! This is what makes the common hard-stop shorthand `#000 0 25%, #fff 0 50%`
//! work: each `0` becomes the previous stop's position.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const W: u32 = 100;
const H: u32 = 100;

fn render(background: &str) -> Vec<u8> {
    let html = format!(
        "<html><body style='margin:0; background:#fff'>\
         <div style='width:100px; height:100px; background:{background}'></div></body></html>"
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
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, W, H, 0, 0),
        W,
        H,
    )
}

fn px(buffer: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [buffer[i], buffer[i + 1], buffer[i + 2]]
}

const BLACK: [u8; 3] = [0, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];

#[test]
fn linear_gradient_stops_behind_an_earlier_position_are_clamped() {
    let b = render("linear-gradient(90deg, #000 0 25%, #fff 0 50%, #000 0 75%, #fff 0)");
    let bands: Vec<_> = [12, 37, 62, 87].iter().map(|&x| px(&b, x, 50)).collect();
    assert_eq!(bands, [BLACK, WHITE, BLACK, WHITE]);
}

#[test]
fn conic_gradient_stops_behind_an_earlier_position_are_clamped() {
    // Quadrants clockwise from the top right.
    let b = render("conic-gradient(#000 0 25%, #fff 0 50%, #000 0 75%, #fff 0)");
    let quadrants: Vec<_> = [(75, 25), (75, 75), (25, 75), (25, 25)]
        .iter()
        .map(|&(x, y)| px(&b, x, y))
        .collect();
    assert_eq!(quadrants, [BLACK, WHITE, BLACK, WHITE]);
}

#[test]
fn repeating_conic_gradient_stops_behind_an_earlier_position_are_clamped() {
    let b = render("repeating-conic-gradient(#000 0 25%, #fff 0 50%)");
    let quadrants: Vec<_> = [(75, 25), (75, 75), (25, 75), (25, 25)]
        .iter()
        .map(|&(x, y)| px(&b, x, y))
        .collect();
    assert_eq!(quadrants, [BLACK, WHITE, BLACK, WHITE]);
}
