//! Scroll containers paint overlay scrollbars when scrolled, fading them
//! out after a delay: for overflow: scroll and overflowing overflow: auto,
//! never for overflow: hidden. At rest nothing is painted, like other
//! overlay scrollbar UIs.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const BLUE: [u8; 3] = [0, 0, 255];

/// Renders `html`, scrolling the `#scroller` element by (dx, dy) first,
/// and returns the pixel at (x, y).
fn pixel(html: &str, scroll: (f64, f64), x: usize, y: usize) -> [u8; 3] {
    pixel_in(html, scroll, x, y, ColorScheme::Light)
}

/// [`pixel`], with the viewport's color scheme chosen by the caller.
fn pixel_in(html: &str, scroll: (f64, f64), x: usize, y: usize, scheme: ColorScheme) -> [u8; 3] {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, scheme)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let scroller = doc.query_selector("#scroller").unwrap().expect("#scroller");
    // Scroll through the scroll API (wheel-delta semantics: negated), so the
    // scroll registers as scrollbar activity like a real user scroll.
    doc.scroll_by(Some(scroller), -scroll.0, -scroll.1, &mut |_| {});
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let idx = (y * 100 + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

fn scroller(overflow: &str, child_height: u32) -> String {
    format!(
        r#"<html><body style="margin:0">
            <div id="scroller" style="width:100px; height:100px; overflow-y:{overflow};">
                <div style="height:{child_height}px; background:#0000ff;"></div>
            </div>
        </body></html>"#
    )
}

#[test]
fn scrolled_auto_scroller_paints_a_thumb() {
    let px = pixel(&scroller("auto", 1000), (0.0, 50.0), 97, 10);
    assert_ne!(px, BLUE, "expected a scrollbar thumb over the content");
}

#[test]
fn unscrolled_unhovered_scroller_paints_no_thumb() {
    let px = pixel(&scroller("auto", 1000), (0.0, 0.0), 97, 4);
    assert_eq!(px, BLUE, "overlay scrollbars are hidden at rest");
}

#[test]
fn non_overflowing_auto_scroller_paints_no_thumb() {
    // A non-overflowing auto container has no scroll range: the scroll
    // doesn't move it and must not summon a thumb.
    let px = pixel(&scroller("auto", 100), (0.0, 10.0), 97, 4);
    assert_eq!(px, BLUE, "no scrollbar for non-overflowing overflow:auto");
}

#[test]
fn hidden_scroller_paints_no_thumb() {
    // overflow:hidden ignores user scrolls entirely.
    let px = pixel(&scroller("hidden", 1000), (0.0, 50.0), 97, 10);
    assert_eq!(px, BLUE, "overflow:hidden must not paint scrollbars");
}

#[test]
fn stale_scroll_offset_alone_paints_no_thumb() {
    // Overlay scrollbar visibility follows scroll *activity*, not scroll
    // position: an offset applied outside the scroll API (no activity)
    // paints nothing, like a scrolled-then-idle container after its
    // scrollbars have faded out.
    let mut doc = HtmlDocument::from_html(
        &scroller("auto", 1000),
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let scroller = doc.query_selector("#scroller").unwrap().expect("#scroller");
    doc.get_node_mut(scroller).unwrap().scroll_offset.y = 50.0;
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let idx = (10 * 100 + 97) * 4;
    let px = [buffer[idx], buffer[idx + 1], buffer[idx + 2]];
    assert_eq!(px, BLUE, "no scrollbar without scroll activity");
}

#[test]
fn horizontal_scroller_paints_a_thumb() {
    let px = pixel(
        r#"<html><body style="margin:0">
            <div id="scroller" style="width:100px; height:100px; overflow-x:auto; overflow-y:hidden;">
                <div style="width:1000px; height:100px; background:#0000ff;"></div>
            </div>
        </body></html>"#,
        (50.0, 0.0),
        10,
        97,
    );
    assert_ne!(px, BLUE, "expected a horizontal scrollbar thumb");
}

#[test]
#[ignore = "scrollbar-color is hardcoded to auto until stylo exposes it to the servo engine (servo/stylo#413)"]
fn scrollbar_color_styles_the_thumb() {
    // Scrolled so the thumb is visible without hover.
    let px = pixel(
        r#"<html><body style="margin:0">
            <div id="scroller" style="width:100px; height:100px; overflow-y:auto; scrollbar-color:#ff0000 transparent;">
                <div style="height:1000px; background:#0000ff;"></div>
            </div>
        </body></html>"#,
        (0.0, 50.0),
        97,
        10,
    );
    assert!(
        px[0] > 200 && px[2] < 100,
        "thumb should be the author's red, got {px:?}"
    );
}

#[test]
#[ignore = "scrollbar-color is hardcoded to auto until stylo exposes it to the servo engine (servo/stylo#413)"]
fn scrollbar_color_styles_the_track() {
    let px = pixel(
        r#"<html><body style="margin:0">
            <div id="scroller" style="width:100px; height:100px; overflow-y:auto; scrollbar-color:#ff0000 #00ff00;">
                <div style="height:1000px; background:#0000ff;"></div>
            </div>
        </body></html>"#,
        (0.0, 50.0),
        97,
        90,
    );
    assert!(
        px[1] > 200 && px[2] < 100,
        "track should be the author's green, got {px:?}"
    );
}

#[test]
#[ignore = "scrollbar-width is hardcoded to auto until stylo exposes it to the servo engine (servo/stylo#413)"]
fn scrollbar_width_none_hides_the_scrollbar() {
    let px = pixel(
        r#"<html><body style="margin:0">
            <div id="scroller" style="width:100px; height:100px; overflow-y:auto; scrollbar-width:none;">
                <div style="height:1000px; background:#0000ff;"></div>
            </div>
        </body></html>"#,
        (0.0, 50.0),
        97,
        10,
    );
    assert_eq!(px, BLUE, "scrollbar-width:none must paint no scrollbar");
}

#[test]
#[ignore = "scrollbar-color is hardcoded to auto until stylo exposes it to the servo engine (servo/stylo#413)"]
fn author_styled_scrollbar_still_hides_at_rest() {
    // scrollbar-color doesn't affect overlay visibility (matches how
    // Firefox/WebKit render the property).
    let px = pixel(
        r#"<html><body style="margin:0">
            <div id="scroller" style="width:100px; height:100px; overflow-y:auto; scrollbar-color:#ff0000 transparent;">
                <div style="height:1000px; background:#0000ff;"></div>
            </div>
        </body></html>"#,
        (0.0, 0.0),
        97,
        4,
    );
    assert_eq!(px, BLUE, "styled overlay scrollbars still hide at rest");
}

#[test]
fn default_thumb_has_a_contrast_stroke() {
    // Default thumbs paint as fill + a thin contrast stroke. The thumb spans
    // x in [88, 98]: x=88 lands on the stroke, x=93 on the fill.
    let html = scroller("auto", 1000);
    let edge = pixel(&html, (0.0, 50.0), 88, 12);
    let fill = pixel(&html, (0.0, 50.0), 93, 12);
    assert_ne!(edge, fill, "thumb edge should carry a contrast stroke");
}

#[test]
fn dark_scheme_uses_a_distinct_thumb() {
    // The default palette follows the viewport color scheme.
    let html = scroller("auto", 1000);
    let light = pixel_in(&html, (0.0, 50.0), 95, 12, ColorScheme::Light);
    let dark = pixel_in(&html, (0.0, 50.0), 95, 12, ColorScheme::Dark);
    assert_ne!(light, dark, "thumb fill should follow the color scheme");
}
