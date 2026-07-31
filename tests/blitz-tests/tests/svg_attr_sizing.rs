//! Sizing of inline `<svg>` elements via their `width`/`height` attributes.
//!
//! Per SVG2, `width` and `height` on an `<svg>` element are *presentation
//! attributes* that map to the CSS `width`/`height` properties, and accept any
//! CSS <length-percentage> (e.g. `1em`, `50%`), resolved with CSS semantics.
//!
//! Regression test: an icon like bbc.co.uk's `<svg width="1em" height="1em">`
//! inside 24px text rendered at 12px because the attributes were never mapped
//! to CSS. Layout fell back to usvg's tree size, where `em` resolves against
//! usvg's default font-size (12px) instead of the element's font-size.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn svg_size(html: &str) -> (f32, f32) {
    let doc = layout_doc(html);
    let svg_id = doc
        .query_selector("#icon")
        .unwrap()
        .expect("#icon not found");
    let layout = doc.get_node(svg_id).unwrap().final_layout();
    (layout.size.width, layout.size.height)
}

#[test]
fn em_dimensions_resolve_against_element_font_size() {
    let size = svg_size(
        r#"<html><body style="margin:0; font-size:24px;">
            <span style="display:inline-block; width:1em; height:1em;">
                <svg id="icon" width="1em" height="1em" viewBox="0 0 32 32"><circle cx="16" cy="16" r="16"/></svg>
            </span>
        </body></html>"#,
    );
    assert_eq!(
        size,
        (24.0, 24.0),
        "1em svg dimensions must resolve against the inherited 24px font-size"
    );
}

#[test]
fn unitless_dimensions_are_user_units() {
    let size = svg_size(
        r#"<html><body style="margin:0;">
            <svg id="icon" width="48" height="32" viewBox="0 0 48 32"></svg>
        </body></html>"#,
    );
    assert_eq!(size, (48.0, 32.0), "unitless svg dimensions are CSS px");
}

#[test]
fn px_dimensions_are_used() {
    let size = svg_size(
        r#"<html><body style="margin:0;">
            <svg id="icon" width="40px" height="20px" viewBox="0 0 32 32"></svg>
        </body></html>"#,
    );
    assert_eq!(size, (40.0, 20.0), "px svg dimensions must be honoured");
}

#[test]
fn percentage_width_resolves_against_containing_block() {
    let size = svg_size(
        r#"<html><body style="margin:0;">
            <div style="width:200px;">
                <svg id="icon" width="50%" height="30" viewBox="0 0 32 32"></svg>
            </div>
        </body></html>"#,
    );
    assert_eq!(
        size,
        (100.0, 30.0),
        "percentage svg width must resolve against the containing block"
    );
}

#[test]
fn css_width_overrides_attributes() {
    // Presentation attributes participate at the lowest cascade level, so
    // author CSS must win over them.
    let size = svg_size(
        r#"<html><body style="margin:0;">
            <svg id="icon" width="10" height="10" viewBox="0 0 32 32" style="width:60px; height:60px;"></svg>
        </body></html>"#,
    );
    assert_eq!(
        size,
        (60.0, 60.0),
        "author CSS must override svg width/height presentation attributes"
    );
}
