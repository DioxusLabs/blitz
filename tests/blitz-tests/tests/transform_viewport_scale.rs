//! Verify that resolved CSS transforms are stored in device-pixel space:
//! translation components (whether from absolute lengths, percentages, or the
//! default `transform-origin: 50% 50%`) are scaled by the viewport scale
//! factor exactly once, while linear components (scale/rotate/skew factors)
//! are not scaled.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn doc_for(css: &str, scale: f32) -> HtmlDocument {
    let html = format!(
        r#"<html><head><style>
        html, body {{ margin: 0; }}
        #box {{ width: 100px; height: 100px; {css} background: red; }}
        </style></head><body><div id="box"></div></body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 400, scale, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn box_transform_coeffs(doc: &HtmlDocument) -> [f64; 6] {
    let box_id = doc.query_selector("#box").unwrap().unwrap();
    doc.get_node(box_id)
        .unwrap()
        .transform()
        .expect("box should have a transform")
        .as_coeffs()
}

fn assert_coeffs_approx_eq(actual: [f64; 6], expected: [f64; 6]) {
    for (a, e) in actual.iter().zip(expected.iter()) {
        assert!(
            (a - e).abs() < 1e-6,
            "expected {expected:?}, got {actual:?}"
        );
    }
}

#[test]
fn px_translate_at_scale_1() {
    let doc = doc_for("transform: translate(100px, 50px);", 1.0);
    assert_coeffs_approx_eq(
        box_transform_coeffs(&doc),
        [1.0, 0.0, 0.0, 1.0, 100.0, 50.0],
    );
}

#[test]
fn px_translate_at_scale_2() {
    // translate(100px, 50px) in CSS pixels = (200, 100) device pixels
    let doc = doc_for("transform: translate(100px, 50px);", 2.0);
    assert_coeffs_approx_eq(
        box_transform_coeffs(&doc),
        [1.0, 0.0, 0.0, 1.0, 200.0, 100.0],
    );
}

#[test]
fn px_translate_property_at_scale_2() {
    let doc = doc_for("translate: 100px 50px;", 2.0);
    assert_coeffs_approx_eq(
        box_transform_coeffs(&doc),
        [1.0, 0.0, 0.0, 1.0, 200.0, 100.0],
    );
}

#[test]
fn pct_translate_property_at_scale_2() {
    // translate: 50% 50% of a 100px box = 50 CSS px = 100 device px
    let doc = doc_for("translate: 50% 50%;", 2.0);
    assert_coeffs_approx_eq(
        box_transform_coeffs(&doc),
        [1.0, 0.0, 0.0, 1.0, 100.0, 100.0],
    );
}

#[test]
fn pct_translate_function_at_scale_2() {
    let doc = doc_for("transform: translate(50%, 50%);", 2.0);
    assert_coeffs_approx_eq(
        box_transform_coeffs(&doc),
        [1.0, 0.0, 0.0, 1.0, 100.0, 100.0],
    );
}

#[test]
fn scale_factor_not_scaled_at_scale_2() {
    // scale(0.5) is unitless: linear components must remain 0.5. It is applied
    // about the default origin (50%, 50%) = (50, 50) CSS px = (100, 100)
    // device px, giving translation components of 100 * (1 - 0.5) = 50.
    let doc = doc_for("transform: scale(0.5);", 2.0);
    assert_coeffs_approx_eq(box_transform_coeffs(&doc), [0.5, 0.0, 0.0, 0.5, 50.0, 50.0]);
}

#[test]
fn default_origin_rotate_at_scale_2() {
    // rotate(90deg) about the default origin (100, 100) device px:
    // [cos, sin, -sin, cos, tx, ty] = [0, 1, -1, 0, 200, 0]
    let doc = doc_for("transform: rotate(90deg);", 2.0);
    assert_coeffs_approx_eq(
        box_transform_coeffs(&doc),
        [0.0, 1.0, -1.0, 0.0, 200.0, 0.0],
    );
}

#[test]
fn px_origin_rotate_at_scale_2() {
    // rotate(180deg) about an absolute-length origin (0, 0):
    // no translation component at any scale.
    let doc = doc_for("transform: rotate(180deg); transform-origin: 0 0;", 2.0);
    assert_coeffs_approx_eq(box_transform_coeffs(&doc), [-1.0, 0.0, 0.0, -1.0, 0.0, 0.0]);
}
