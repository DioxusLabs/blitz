//! CSS `opacity` does not inherit, but a `<g opacity>`'s value must still visually apply to everything painted inside it.
//! Regression test for a bug where the flat per-node paint loop opened and closed an opacity layer around the *group's own*
//! paint call, leaving descendants, separate iterations of the same flat loop, painted at full opacity regardless of any
//! ancestor group's opacity.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn center_pixel(html: &str) -> [u8; 3] {
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
        |scene| paint_scene(scene, &mut doc, 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let idx = (50 * 100 + 50) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

#[test]
fn group_opacity_blends_its_descendant_shape_with_the_background() {
    let px = center_pixel(
        r##"<html><body style="margin:0; background:#ffffff;">
            <svg width="100" height="100" viewBox="0 0 100 100">
                <g opacity="0.5">
                    <rect x="0" y="0" width="100" height="100" fill="#ff0000"/>
                </g>
            </svg>
        </body></html>"##,
    );
    // 50% red over a white background: full opacity would read as pure red;
    // ignoring the group's opacity entirely (the bug) produces exactly that.
    // Blended, red stays saturated while green/blue rise partway back towards white.
    assert_ne!(px, [255, 0, 0], "group opacity must not be ignored");
    assert_eq!(
        px[0], 255,
        "red channel is already saturated in both layers"
    );
    assert!(
        px[1] > 100 && px[1] < 200,
        "green channel should sit roughly halfway between 0 (red) and 255 (white), got {}",
        px[1]
    );
    assert_eq!(
        px[1], px[2],
        "the blend is achromatic on the green/blue axes"
    );
}

#[test]
fn nested_group_opacity_compounds_multiplicatively() {
    let single = center_pixel(
        r##"<html><body style="margin:0; background:#ffffff;">
            <svg width="100" height="100" viewBox="0 0 100 100">
                <g opacity="0.5">
                    <rect x="0" y="0" width="100" height="100" fill="#ff0000"/>
                </g>
            </svg>
        </body></html>"##,
    );
    let nested = center_pixel(
        r##"<html><body style="margin:0; background:#ffffff;">
            <svg width="100" height="100" viewBox="0 0 100 100">
                <g opacity="0.5">
                    <g opacity="0.5">
                        <rect x="0" y="0" width="100" height="100" fill="#ff0000"/>
                    </g>
                </g>
            </svg>
        </body></html>"##,
    );
    // 0.5 * 0.5 = 0.25 total coverage, lighter (closer to white) than a single 0.5 group.
    assert!(
        nested[1] > single[1],
        "two nested 0.5-opacity groups (0.25 total) should be lighter than one 0.5 group, \
         got nested={:?} single={:?}",
        nested,
        single
    );
}
