//! CSS and presentation-attribute SVG paint properties must reach usvg.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn pixel(html: &str, x: usize, y: usize) -> [u8; 3] {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc.as_mut(), 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let index = (y * 100 + x) * 4;
    [buffer[index], buffer[index + 1], buffer[index + 2]]
}

#[test]
fn css_currentcolor_fill_is_serialized() {
    let px = pixel(
        r##"<html><head><style>
            #icon { display:block; width:100px; height:100px; color:#0071f1; fill:currentcolor; }
        </style></head><body style="margin:0">
            <svg id="icon" viewBox="0 0 100 100">
                <path d="M50 10 L90 90 L10 90 Z"/>
            </svg>
        </body></html>"##,
        50,
        50,
    );
    assert_eq!(px, [0, 113, 241]);
}

#[test]
fn css_fill_overrides_presentation_attribute() {
    let px = pixel(
        r##"<html><head><style>
            #icon { display:block; width:100px; height:100px; fill:#00ff00; }
        </style></head><body style="margin:0">
            <svg id="icon" fill="#ff0000" viewBox="0 0 100 100">
                <path d="M50 10 L90 90 L10 90 Z"/>
            </svg>
        </body></html>"##,
        50,
        50,
    );
    assert_eq!(px, [0, 255, 0]);
}

#[test]
fn fill_none_is_preserved() {
    let px = pixel(
        r##"<html><body style="margin:0; background:#0000ff">
            <svg id="icon" style="display:block; width:100px; height:100px" fill="none" viewBox="0 0 100 100">
                <path d="M50 10 L90 90 L10 90 Z"/>
            </svg>
        </body></html>"##,
        50,
        50,
    );
    assert_eq!(px, [0, 0, 255]);
}

#[test]
fn stroke_and_stroke_width_are_serialized() {
    let px = pixel(
        r##"<html><body style="margin:0">
            <svg id="icon" style="display:block; width:100px; height:100px" fill="none"
                stroke="#0000ff" stroke-width="10" viewBox="0 0 100 100">
                <path d="M10 10 H90 V90 H10 Z"/>
            </svg>
        </body></html>"##,
        6,
        50,
    );
    assert_eq!(px, [0, 0, 255]);
    assert_eq!(
        pixel(
            r##"<html><body style="margin:0">
                <svg style="display:block; width:100px; height:100px" fill="none"
                    stroke="#0000ff" stroke-width="1" viewBox="0 0 100 100">
                    <path d="M10 10 H90 V90 H10 Z"/>
                </svg>
            </body></html>"##,
            8,
            50,
        ),
        [0, 0, 0],
        "the 10-unit stroke must extend beyond the 1-unit default stroke"
    );
}

#[test]
fn css_paint_server_fragment_stays_local() {
    let px = pixel(
        r##"<html><head><style>
            #icon { display:block; width:100px; height:100px; fill:url(#gradient); }
        </style></head><body style="margin:0">
            <svg id="icon" viewBox="0 0 100 100">
                <defs>
                    <linearGradient id="gradient">
                        <stop offset="0" stop-color="#ff0000"/>
                        <stop offset="1" stop-color="#0000ff"/>
                    </linearGradient>
                </defs>
                <path d="M0 0 H100 V100 H0 Z"/>
            </svg>
        </body></html>"##,
        1,
        50,
    );
    assert!(
        px[0] > px[2],
        "local CSS paint server should resolve to red"
    );
}

#[test]
fn presentation_paint_server_fragment_stays_local() {
    let px = pixel(
        r##"<html><body style="margin:0">
            <svg style="display:block; width:100px; height:100px"
                fill="url(#gradient)" viewBox="0 0 100 100">
                <defs>
                    <linearGradient id="gradient">
                        <stop offset="0" stop-color="#ff0000"/>
                        <stop offset="1" stop-color="#0000ff"/>
                    </linearGradient>
                </defs>
                <path d="M0 0 H100 V100 H0 Z"/>
            </svg>
        </body></html>"##,
        1,
        50,
    );
    assert!(
        px[0] > px[2],
        "local presentation paint server should resolve to red"
    );
}
