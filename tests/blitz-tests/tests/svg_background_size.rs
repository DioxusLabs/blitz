//! `background-size: auto` sizing for SVG background images.
//!
//! An SVG that declares a `viewBox` but no (or percentage) `width`/`height`
//! has only an intrinsic aspect ratio, not intrinsic dimensions. Per the CSS
//! default sizing algorithm it must be scaled to *contain* the background
//! positioning area rather than painted at its `viewBox` pixel size (the bug
//! that made the crowdsupply.com `.navbar-logo` render at ~2x).

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_dom::node::{ImageData, SvgImageData};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

/// Renders a 100x100 div whose `background` is set to the given shorthand and
/// whose loaded background image is the provided SVG source. `intrinsic_*`
/// mirror what `parse_svg_image` would detect for the SVG. Returns the pixel
/// at (x, y). The div sits on a solid blue page background.
fn pixel(
    background: &str,
    svg_src: &str,
    intrinsic_width: Option<f32>,
    intrinsic_height: Option<f32>,
    x: usize,
    y: usize,
) -> [u8; 3] {
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
        let tree =
            usvg::Tree::from_str(svg_src, &usvg::Options::default()).expect("valid test SVG");
        let svg = SvgImageData {
            tree: Arc::new(tree),
            intrinsic_width,
            intrinsic_height,
        };
        let node = doc.get_node_mut(box_id).unwrap();
        let el = node.element_data_mut().unwrap();
        for layer in el.background_images.iter_mut().flatten() {
            layer.status = blitz_dom::node::Status::Ok;
            layer.image = ImageData::Svg(svg.clone());
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

// A 2:1 red SVG with a viewBox but no width/height: intrinsic aspect ratio
// only, no intrinsic dimensions.
const VIEWBOX_ONLY: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><rect width="200" height="100" fill="red"/></svg>"#;

// A 50x50 red SVG that declares absolute width/height: has intrinsic size.
const WITH_SIZE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50" viewBox="0 0 50 50"><rect width="50" height="50" fill="red"/></svg>"#;

#[test]
fn viewbox_only_svg_is_contained_not_intrinsic() {
    // 2:1 aspect ratio contained in a 100x100 box -> 100x50 at the top-left
    // (default background-position). The top half is red; the bottom half must
    // show the page background. Before the fix the SVG was painted at its
    // 200x100 viewBox size and filled the whole box.
    let top = pixel(
        "url('https://example.com/x.svg') no-repeat",
        VIEWBOX_ONLY,
        None,
        None,
        50,
        25,
    );
    assert_eq!(top, RED, "contained SVG must cover the top of the box");

    let bottom = pixel(
        "url('https://example.com/x.svg') no-repeat",
        VIEWBOX_ONLY,
        None,
        None,
        50,
        75,
    );
    assert_ne!(
        bottom, RED,
        "viewBox-only SVG must be contained (letterboxed), not painted at 2x"
    );
}

#[test]
fn svg_with_intrinsic_size_uses_it() {
    // A 50x50 SVG with `background-size: auto` renders at 50x50 at the
    // top-left, leaving the rest of the 100x100 box as page background.
    let inside = pixel(
        "url('https://example.com/x.svg') no-repeat",
        WITH_SIZE,
        Some(50.0),
        Some(50.0),
        25,
        25,
    );
    assert_eq!(
        inside, RED,
        "intrinsically-sized SVG must paint at its size"
    );

    let outside = pixel(
        "url('https://example.com/x.svg') no-repeat",
        WITH_SIZE,
        Some(50.0),
        Some(50.0),
        75,
        75,
    );
    assert_ne!(
        outside, RED,
        "a 50x50 SVG must not fill the whole 100x100 box"
    );
}

#[test]
fn viewbox_only_svg_respects_explicit_contain() {
    // `background-size: contain` gives the same result as auto for a
    // viewBox-only SVG: 100x50 at the top-left.
    let bottom = pixel(
        "url('https://example.com/x.svg') 0 0/contain no-repeat",
        VIEWBOX_ONLY,
        None,
        None,
        50,
        75,
    );
    assert_ne!(bottom, RED, "contain must letterbox the 2:1 SVG");
}
