//! A `calc()` that is not linear in its basis resolves against each basis, as
//! Chrome resolves it: `text-indent: max(20px, 10%)` is 20px in a 100px block
//! and 40px in a 400px one, and `padding-inline-start: calc(10px - 50%)` is
//! 2px in a 16px block and clamps at 0 in a 400px one.

#![cfg(all(feature = "winkin", not(feature = "parley")))]

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

/// Returns the left edge of `#x` in `body`, laid out in a 600px viewport.
fn left(body: &str) -> f32 {
    let html = format!(
        r#"<!DOCTYPE html><style>
            body {{ margin: 0; font: 16px monospace; }}
        </style>{body}"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(600, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let x = doc.query_selector("#x").unwrap().unwrap();
    doc.get_client_bounding_rect(x).unwrap().x as f32
}

#[test]
fn a_max_text_indent_takes_the_larger_at_each_width() {
    for (width, indent) in [(100, 20.0), (400, 40.0)] {
        let body = format!(
            r#"<div style="width: {width}px; text-indent: max(20px, 10%)"><span id="x">x</span></div>"#
        );
        assert_eq!(left(&body), indent, "{width}px wide");
    }
}

#[test]
fn a_padding_below_zero_clamps_at_zero() {
    for (width, padding) in [(16, 2.0), (400, 0.0)] {
        let body = format!(
            r#"<div style="width: {width}px"><span style="padding-inline-start: calc(10px - 50%)"><span id="x">x</span></span></div>"#
        );
        assert_eq!(left(&body), padding, "{width}px wide");
    }
}
