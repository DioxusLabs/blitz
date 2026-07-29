//! Sizing of block-level replaced elements.
//!
//! Block layout stretch-sizes in-flow children to the container width, but
//! block-level replaced elements are exempt: with `width: auto` they use their
//! intrinsic size (https://www.w3.org/TR/CSS22/visudet.html#block-replaced-width).
//!
//! Regression test: GitHub's stylesheet applies `img { max-width: 100%;
//! display: block }` to README content, which made badge/logo images stretch
//! to the full width of their container.

use blitz_dom::DocumentConfig;
use blitz_dom::node::{ImageData, SpecialElementData, SvgImageData};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

// A 142x20 badge-like SVG with intrinsic dimensions.
const BADGE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="142" height="20" viewBox="0 0 142 20"><rect width="142" height="20" fill="red"/></svg>"#;

/// Lays out `html` with the badge SVG injected as the loaded image of `#img`
/// and returns the image element's final size.
fn img_size(html: &str) -> (f32, f32) {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    let img_id = doc.query_selector("#img").unwrap().expect("#img");
    {
        let tree =
            usvg::Tree::from_str(BADGE_SVG, &usvg::Options::default()).expect("valid test SVG");
        let svg = SvgImageData {
            tree: Arc::new(tree),
            intrinsic_width: Some(142.0),
            intrinsic_height: Some(20.0),
        };
        let node = doc.get_node_mut(img_id).unwrap();
        node.element_data_mut().unwrap().special_data =
            SpecialElementData::Image(Box::new(ImageData::Svg(svg)));
    }
    doc.resolve(0.0);
    let layout = doc.get_node(img_id).unwrap().final_layout();
    (layout.size.width, layout.size.height)
}

#[test]
fn block_img_with_auto_width_uses_intrinsic_size() {
    let size = img_size(
        r#"<html><body style="margin:0;">
            <div style="width:600px;">
                <img id="img" style="display:block; max-width:100%;">
            </div>
        </body></html>"#,
    );
    assert_eq!(
        size,
        (142.0, 20.0),
        "block-level replaced element must not be stretched to the container width"
    );
}

#[test]
fn block_img_intrinsic_size_is_clamped_by_max_width() {
    let size = img_size(
        r#"<html><body style="margin:0;">
            <div style="width:100px;">
                <img id="img" style="display:block; max-width:100%;">
            </div>
        </body></html>"#,
    );
    let expected_height = 100.0 * 20.0 / 142.0;
    assert_eq!(size.0, 100.0);
    assert!(
        (size.1 - expected_height).abs() <= 0.5,
        "max-width must still clamp the intrinsic size, preserving the aspect ratio \
         (expected height ~{expected_height}, got {})",
        size.1
    );
}

#[test]
fn block_img_with_specified_width_uses_it() {
    let size = img_size(
        r#"<html><body style="margin:0;">
            <div style="width:600px;">
                <img id="img" style="display:block; width:300px; height:50px;">
            </div>
        </body></html>"#,
    );
    assert_eq!(size, (300.0, 50.0));
}
