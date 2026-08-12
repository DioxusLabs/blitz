//! Integration tests for first-party inline SVG.
//!
//! Mirrors the harness pattern in `svg_attr_sizing.rs`. These specifically exercise sizing behaviour that is shared
//! between the `usvg` and `svg-native` paths so they double as a parity check: the observable box size of a root
//! `<svg>` must not change based on which rendering backend produced it.

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
fn default_viewport_is_300x150() {
    let size = svg_size(r#"<html><body style="margin:0;"><svg id="icon"></svg></body></html>"#);
    assert_eq!(size, (300.0, 150.0));
}

#[test]
fn explicit_dimensions_are_honoured() {
    let size = svg_size(
        r#"<html><body style="margin:0;">
            <svg id="icon" width="48" height="32" viewBox="0 0 48 32"></svg>
        </body></html>"#,
    );
    assert_eq!(size, (48.0, 32.0));
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
    assert_eq!(size, (100.0, 30.0));
}

#[test]
fn css_width_overrides_presentation_attributes() {
    let size = svg_size(
        r#"<html><body style="margin:0;">
            <svg id="icon" width="10" height="10" viewBox="0 0 32 32" style="width:60px; height:60px;"></svg>
        </body></html>"#,
    );
    assert_eq!(size, (60.0, 60.0));
}

#[test]
fn deeply_nested_shapes_do_not_panic_construction_or_layout() {
    let _doc = layout_doc(
        r##"<html><body style="margin:0;">
            <svg id="icon" width="200" height="100" viewBox="0 0 200 100">
                <defs>
                    <rect id="tpl" width="10" height="10" fill="red"/>
                </defs>
                <g transform="translate(10,10)">
                    <rect x="0" y="0" width="50" height="30" rx="5" fill="blue"/>
                    <circle cx="80" cy="20" r="15" fill="green" stroke="black" stroke-width="2"/>
                    <ellipse cx="120" cy="20" rx="20" ry="10"/>
                    <line x1="0" y1="50" x2="50" y2="50" stroke="red"/>
                    <polyline points="0,60 10,70 20,60" fill="none" stroke="purple"/>
                    <polygon points="30,60 40,70 50,60" fill="orange"/>
                    <path d="M0,80 L20,80 L10,90 Z" fill="yellow"/>
                    <use href="#tpl" x="150" y="0"/>
                    <text x="0" y="95" fill="black">hello</text>
                </g>
            </svg>
        </body></html>"##,
    );
}

#[test]
fn nested_svg_establishes_its_own_viewport() {
    use blitz_dom::svg::SvgNodeKind;
    use kurbo::Point;

    let doc = layout_doc(
        r##"<html><body style="margin:0;">
            <svg id="icon" width="100" height="100">
                <svg x="10" y="20" width="50" height="50" viewBox="0 0 25 25">
                    <rect x="0" y="0" width="25" height="25" fill="blue"/>
                </svg>
            </svg>
        </body></html>"##,
    );
    let svg_id = doc.query_selector("#icon").unwrap().unwrap();
    let ctx = doc
        .get_node(svg_id)
        .unwrap()
        .element_data()
        .unwrap()
        .svg_root_data()
        .expect("svg root should have a constructed SvgContext");

    let rect = ctx
        .nodes
        .iter()
        .find(|n| matches!(n.kind, SvgNodeKind::Shape(_)))
        .expect("nested <rect> should still be walked into the flat node list");

    let origin = rect.ctm * Point::new(0.0, 0.0);
    let corner = rect.ctm * Point::new(25.0, 25.0);
    assert_eq!(origin, Point::new(10.0, 20.0));
    assert_eq!(corner, Point::new(60.0, 70.0));
}

#[test]
fn malformed_attributes_degrade_gracefully() {
    let _doc = layout_doc(
        r##"<html><body style="margin:0;">
            <svg id="icon" width="not-a-number" viewBox="bogus">
                <rect width="-5" height="abc" fill="not-a-color"/>
                <circle r="-10"/>
                <use href="#does-not-exist"/>
            </svg>
        </body></html>"##,
    );
}

#[test]
fn use_self_reference_cycle_does_not_hang_or_panic() {
    // A <use> that (transitively) targets itself must be caught by the cycle guard, not recurse forever.
    let _doc = layout_doc(
        r##"<html><body style="margin:0;">
            <svg id="icon" width="100" height="100" viewBox="0 0 100 100">
                <g id="a"><use href="#a"/></g>
            </svg>
        </body></html>"##,
    );
}

#[test]
fn use_targeting_own_ancestor_is_caught_immediately_by_the_ancestor_guard() {
    let doc = layout_doc(
        r##"<html><body style="margin:0;">
            <svg id="icon" width="100" height="100" viewBox="0 0 100 100">
                <g id="a"><use href="#a"/></g>
            </svg>
        </body></html>"##,
    );
    let svg_id = doc.query_selector("#icon").unwrap().unwrap();
    let ctx = doc
        .get_node(svg_id)
        .unwrap()
        .element_data()
        .unwrap()
        .svg_root_data()
        .expect("svg root should have a constructed SvgContext");
    assert_eq!(
        ctx.nodes.len(),
        1,
        "ancestor-cycle guard should reject the <use> on its first attempt, \
         not expand it MAX_REF_DEPTH times before the depth-cap backstop kicks in"
    );
}
