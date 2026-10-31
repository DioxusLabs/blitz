//! Min/max constraints on a replaced element with an intrinsic aspect ratio.
//!
//! The ratio-preserving resolution table in CSS 2.2 §10.4 only applies when
//! both `width` and `height` are `auto`. When one axis is specified, that axis
//! keeps its specified size (clamped by its own min/max), and only an `auto`
//! axis is derived from the other through the ratio (§10.3.2, §10.6.2).
//!
//! Regression test: an image with `min-width: 600px; height: 0` (a common
//! "force the table to 600px" pattern in HTML email) was laid out
//! `600 x 600 / ratio` instead of `600 x 0`.
//!
//! `<canvas>` is used because its `width`/`height` attributes set its
//! intrinsic size (and so its ratio) without being specified CSS sizes, and
//! it needs no resource to load. Expected sizes are what Chromium produces
//! for the same markup.

use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            font_ctx: Some(FontContext::new()),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn size_of(doc: &HtmlDocument, selector: &str) -> (f32, f32) {
    let id = doc.query_selector(selector).unwrap().unwrap();
    let size = doc.get_node(id).unwrap().final_layout().size;
    (size.width, size.height)
}

fn canvas(style: &str) -> (f32, f32) {
    let doc = layout_doc(&format!(
        r#"<html><body style="margin:0"><canvas id="r" width="10" height="5" style="display:block; {style}"></canvas></body></html>"#
    ));
    size_of(&doc, "#r")
}

#[test]
fn specified_height_is_kept_when_min_width_applies() {
    assert_eq!(canvas("min-width:600px; height:0"), (600.0, 0.0));
}

#[test]
fn specified_width_is_kept_when_min_height_applies() {
    assert_eq!(canvas("width:100px; min-height:200px"), (100.0, 200.0));
}

#[test]
fn specified_height_is_kept_when_max_width_applies() {
    assert_eq!(canvas("height:20px; max-width:30px"), (30.0, 20.0));
}

#[test]
fn auto_axis_follows_the_clamped_specified_axis() {
    // width is specified but below min-width; height is auto, so it is
    // derived from the used (clamped) width.
    assert_eq!(canvas("width:100px; min-width:300px"), (300.0, 150.0));
}

#[test]
fn both_auto_still_preserves_the_ratio() {
    // Unchanged behaviour: the §10.4 table applies when both axes are auto.
    assert_eq!(canvas("min-width:300px"), (300.0, 150.0));
}
