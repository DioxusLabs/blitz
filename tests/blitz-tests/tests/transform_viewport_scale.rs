//! Verify that CSS `transform: translate(...)` is correctly scaled by the
//! viewport scale factor (hidpi_scale * zoom).
//!
//! Per the CSS spec, transform values are in CSS pixels and should be multiplied
//! by the device pixel ratio when rendering to physical pixels. If the
//! transform is not scaled, a `translate(100px)` on a hidpi-2 screen moves
//! 100 physical pixels instead of 200, causing misalignment between layout
//! (which IS scaled) and transforms (which are not).

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    html, body { margin: 0; }
    #box {
        width: 100px;
        height: 100px;
        transform: translate(100px, 50px);
        background: red;
    }
</style></head>
<body><div id="box"></div></body></html>
"#;

fn make_doc(width: u32, height: u32, scale: f32) -> HtmlDocument {
    HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(width, height, scale, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    )
}

/// Read the resolved CSS transform coefficients for `#box`.
/// Returns `Some([m11, m12, m21, m22, m31, m32])` or `None` if no transform.
fn box_transform_coeffs(doc: &HtmlDocument) -> Option<[f64; 6]> {
    let box_id = doc.query_selector("#box").unwrap().unwrap();
    let node = doc.get_node(box_id).unwrap();
    node.transform().map(|t| t.as_coeffs())
}

/// Layout location of `#box` (before transform).
fn box_layout_location(doc: &HtmlDocument) -> (f32, f32) {
    let box_id = doc.query_selector("#box").unwrap().unwrap();
    let layout = &doc.get_node(box_id).unwrap().final_layout();
    (layout.location.x, layout.location.y)
}

#[test]
fn transform_translate_at_scale_1() {
    let mut doc = make_doc(400, 400, 1.0);
    doc.resolve(0.0);

    let t = box_transform_coeffs(&doc).expect("box should have a transform");
    // translate(100px, 50px) => Affine([1, 0, 0, 1, 100, 50])
    assert_eq!(t, [1.0, 0.0, 0.0, 1.0, 100.0, 50.0]);
}

#[test]
fn transform_translate_at_scale_2() {
    let mut doc = make_doc(400, 400, 2.0);
    doc.resolve(0.0);

    let (lx, ly) = box_layout_location(&doc);
    // Layout is in CSS pixels, unscaled by the viewport scale.
    assert_eq!(lx, 0.0);
    assert_eq!(ly, 0.0);

    let t = box_transform_coeffs(&doc).expect("box should have a transform");
    // At hidpi_scale=2, translate(100px, 50px) in CSS pixels should become
    // translate(200px, 100px) in device pixels.
    //
    // If this assertion fails with [1, 0, 0, 1, 100, 50], it means the
    // transform was NOT scaled by the viewport factor -- a bug.
    assert_eq!(
        t,
        [1.0, 0.0, 0.0, 1.0, 200.0, 100.0],
        "transform translate should be scaled by viewport scale factor"
    );
}

#[test]
fn transform_scale_at_scale_2() {
    // At hidpi_scale=2, a CSS scale(0.5) should still be scale(0.5) --
    // scale factors are unitless and should NOT change with viewport scale.
    // But the *effect* on screen should be as if applied after the
    // viewport scale, so the combined scale is 2.0 * 0.5 = 1.0.
    let html = r#"<html><head><style>
        html, body { margin: 0; }
        #box { width: 100px; height: 100px; transform: scale(0.5); background: red; }
    </style></head><body><div id="box"></div></body></html>"#;

    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 400, 2.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    let box_id = doc.query_selector("#box").unwrap().unwrap();
    let node = doc.get_node(box_id).unwrap();
    let t = node.transform().expect("box should have a transform");

    // The CSS transform matrix itself: scale(0.5) => [0.5, 0, 0, 0.5, 0, 0]
    // This is the raw CSS transform without viewport scaling applied.
    let coeffs = t.as_coeffs();
    eprintln!("scale(0.5) at hidpi=2: coeffs = {:?}", coeffs);
    // We expect the matrix to represent 0.5 scale (unscaled by viewport)
    // because scale is unitless. The viewport scaling should be applied
    // separately at paint time.
    assert!(
        (coeffs[0] - 0.5).abs() < 1e-6 && (coeffs[3] - 0.5).abs() < 1e-6,
        "scale factor should be 0.5 regardless of viewport scale"
    );
}
