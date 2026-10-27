//! SVG image layers use the CSS background positioning and repetition rules.
use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_dom::node::{ImageData, SvgImageData};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const TILE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><path d="M0 0H10V20H0Z" fill="red"/><path d="M10 0H20V20H10Z" fill="blue"/></svg>"#;
const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

fn document(css: &str, svg: &str, scale: f32) -> HtmlDocument {
    let html = format!(
        "<html><body style='margin:0;background:white'><div id='box' style='\
         position:absolute;left:20px;top:20px;width:100px;height:100px;\
         background-image:url(https://example.com/tile.svg);{css}'></div></body></html>"
    );
    let size = (160.0 * scale) as u32;
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(size, size, scale, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let id = doc.query_selector("#box").unwrap().unwrap();
    let svg = SvgImageData::from_data(svg.as_bytes(), &usvg::Options::default()).unwrap();
    for layer in doc
        .get_node_mut(id)
        .unwrap()
        .element_data_mut()
        .unwrap()
        .background_images
        .iter_mut()
        .flatten()
    {
        layer.status = blitz_dom::node::Status::Ok;
        layer.image = ImageData::Svg(svg.clone());
    }
    doc.resolve(0.0);
    doc
}

fn check(css: &str, svg: &str, probes: &[(usize, usize, [u8; 3])]) {
    for scale in [0.5, 1.0, 2.0] {
        let mut doc = document(css, svg, scale);
        let size = (160.0 * scale) as u32;
        let pixels = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| paint_scene(scene, doc.as_mut(), scale as f64, size, size, 0, 0),
            size,
            size,
        );
        for &(x, y, expected) in probes {
            let index =
                ((y as f32 * scale) as usize * size as usize + (x as f32 * scale) as usize) * 4;
            assert_eq!(
                &pixels[index..index + 3],
                &expected,
                "{css}, scale {scale}, pixel ({x}, {y})"
            );
        }
    }
}

#[test]
fn svg_repeat_modes_respect_position_and_element_offset() {
    for (repeat, far_x, far_y, far_both) in [
        ("repeat", RED, RED, RED),
        ("repeat-x", RED, WHITE, WHITE),
        ("repeat-y", WHITE, RED, WHITE),
        ("no-repeat", WHITE, WHITE, WHITE),
    ] {
        check(
            &format!("background-repeat:{repeat};background-position:10px 10px"),
            TILE,
            &[
                (34, 34, RED),
                (44, 34, BLUE),
                (74, 34, far_x),
                (34, 74, far_y),
                (74, 74, far_both),
                (124, 34, WHITE),
            ],
        );
    }
}

#[test]
fn svg_space_uses_gaps_and_positions_a_single_tile() {
    check(
        "background-repeat:space;background-size:30px 30px",
        TILE,
        &[
            (24, 24, RED),
            (44, 24, BLUE),
            (52, 24, WHITE),
            (58, 58, RED),
            (114, 114, BLUE),
        ],
    );
    check(
        "background-repeat:space;background-size:60px 60px;background-position:center",
        TILE,
        &[
            (24, 24, WHITE),
            (44, 44, RED),
            (84, 84, BLUE),
            (104, 104, WHITE),
        ],
    );
}

#[test]
fn svg_round_resizes_tiles_and_auto_axis_before_positioning() {
    // 30px rounds to 100/3. The red/blue boundary moves from 15px to 16.67px.
    check(
        "background-repeat:round;background-size:30px 30px",
        TILE,
        &[(74, 74, BLUE), (92, 92, RED), (114, 114, BLUE)],
    );
    // Oversized tiles still round to one tile, not a division by zero.
    check(
        "background-repeat:round;background-size:300px 300px",
        TILE,
        &[(24, 24, RED), (74, 74, BLUE), (114, 114, BLUE)],
    );
    // Rounding 40 to 100/3 rescales the auto height to 100/3; centered y is 53.33.
    check(
        "background-repeat:round no-repeat;background-size:40px auto;background-position:center",
        TILE,
        &[
            (24, 50, WHITE),
            (24, 56, RED),
            (24, 84, RED),
            (24, 88, WHITE),
        ],
    );
}

#[test]
fn svg_repetition_covers_clip_box_and_follows_css_transform() {
    check(
        "width:80px;height:80px;padding:10px;background-origin:content-box;background-clip:padding-box;transform:translate(10px, 10px)",
        TILE,
        &[
            (24, 44, WHITE),
            (34, 44, BLUE),
            (44, 44, RED),
            (114, 114, BLUE),
            (124, 124, RED),
            (134, 44, WHITE),
        ],
    );
}

#[test]
fn each_svg_tile_clips_artwork_to_its_viewport() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect x="-20" y="-20" width="60" height="60" fill="red"/></svg>"#;
    check(
        "background-repeat:space;background-size:30px 30px",
        svg,
        &[(24, 24, RED), (52, 24, WHITE), (58, 58, RED)],
    );
}

#[test]
fn zero_and_near_zero_svg_tiles_do_not_generate_unbounded_scenes() {
    for css in [
        "background-size:0 20px",
        "background-size:20px 0",
        "background-size:.01px .01px",
        "background-size:.01px .01px;background-repeat:space",
    ] {
        let mut doc = document(css, TILE, 1.0);
        let mut scene = anyrender::Scene::new();
        paint_scene(&mut scene, doc.as_mut(), 1.0, 160, 160, 0, 0);
        assert!(
            scene.commands.len() < 100,
            "{css}: {} commands",
            scene.commands.len()
        );
    }
}
