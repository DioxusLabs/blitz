//! Min and max sizes of a replaced element whose width or height is set.
//!
//! CSS 2.2 §10.4's table, which keeps the aspect ratio while resolving min/max violations, is
//! for a replaced element whose width and height are both `auto`. With one of them set, that
//! axis keeps its value (within its own min and max) and only the `auto` one follows the ratio.
//!
//! Regression test: Shopify mail opens with a spacer image styled `min-width:600px; height:0`,
//! which came out 600×600 instead of 600×0.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn size_of(html: &str) -> (f32, f32) {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let id = doc.query_selector("#r").unwrap().expect("#r not found");
    let layout = doc.get_node(id).unwrap().final_layout();
    (layout.size.width, layout.size.height)
}

fn square(style: &str) -> String {
    format!(
        r#"<body style="margin:0"><svg id="r" viewBox="0 0 8 8" style="display:block;{style}"><rect width="8" height="8"/></svg></body>"#
    )
}

#[test]
fn a_set_height_stays_beside_a_larger_min_width() {
    assert_eq!(size_of(&square("min-width:300px;height:0")), (300.0, 0.0));
    assert_eq!(
        size_of(&square("min-width:300px;height:10px")),
        (300.0, 10.0)
    );
}

#[test]
fn a_set_width_stays_beside_a_larger_min_height() {
    assert_eq!(
        size_of(&square("min-height:300px;width:10px")),
        (10.0, 300.0)
    );
}

#[test]
fn the_auto_axis_follows_the_ratio_from_the_clamped_one() {
    // A set width held to its max-width: the auto height follows the width used.
    assert_eq!(
        size_of(&square("width:400px;max-width:100px")),
        (100.0, 100.0)
    );
}

#[test]
fn both_auto_keeps_the_ratio() {
    // Without a size of its own it fills the width; held to a max-width, it stays square.
    assert_eq!(size_of(&square("max-width:100px")), (100.0, 100.0));
}
