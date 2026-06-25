//! Scroll containers paint overlay scrollbars while hovered or scrolled:
//! always for overflow: scroll, only when overflowing for overflow: auto,
//! never for overflow: hidden. At rest (unhovered, unscrolled) nothing is
//! painted, like other overlay scrollbar UIs.

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
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let scroller = doc.query_selector("#scroller").unwrap().expect("#scroller");
    let node = doc.get_node_mut(scroller).unwrap();
    node.scroll_offset.x = scroll.0;
    node.scroll_offset.y = scroll.1;
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
    // Even with a (stale) scroll offset, a non-overflowing auto container
    // has no scroll range and paints no thumb.
    let px = pixel(&scroller("auto", 100), (0.0, 10.0), 97, 4);
    assert_eq!(px, BLUE, "no scrollbar for non-overflowing overflow:auto");
}

#[test]
fn hidden_scroller_paints_no_thumb() {
    let px = pixel(&scroller("hidden", 1000), (0.0, 50.0), 97, 10);
    assert_eq!(px, BLUE, "overflow:hidden must not paint scrollbars");
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
