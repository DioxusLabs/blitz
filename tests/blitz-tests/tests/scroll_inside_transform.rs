//! A scroll container's scroll offset moves its contents in the container's own
//! coordinate space, inside its CSS transform. Paint and hit testing must both
//! apply the transform to the scrolled contents, not scroll them in page space.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{DocumentConfig, ScrollBehavior};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 160;

/// A scroller scaled 2x from its top-left corner at (20, 20). Its green target
/// sits at x = 50..150 in the scroller's coordinates, past its 40px width.
fn scrolled_scaled_scroller() -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        r#"<html><body style="margin:0;background:white">
        <div id=scroller style="position:absolute;left:20px;top:20px;width:40px;height:40px;
            overflow:hidden;transform:scale(2);transform-origin:0 0;background:red">
          <div id=target style="position:absolute;left:50px;top:10px;width:100px;height:20px;background:#00cc00"></div>
        </div>
        </body></html>"#,
        DocumentConfig {
            viewport: Some(Viewport::new(WIDTH, HEIGHT, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let scroller = doc.get_element_by_id("scroller").unwrap();
    doc.scroll_to(scroller, 50.0, 0.0, ScrollBehavior::Instant);
    assert_eq!(doc.get_node(scroller).unwrap().scroll_offset().x, 50.0);
    doc
}

fn pixel(doc: &mut HtmlDocument, x: usize, y: usize) -> [u8; 3] {
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc, 1.0, WIDTH, HEIGHT, 0, 0),
        WIDTH,
        HEIGHT,
    );
    let idx = (y * WIDTH as usize + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

// Scrolled by 50px, the target's left edge is at the scroller's left edge, so on
// the page it starts at x = 20 and covers the whole 80px wide scaled scroller.

#[test]
fn scrolled_content_paints_inside_the_transform() {
    let mut doc = scrolled_scaled_scroller();
    assert_eq!(pixel(&mut doc, 30, 50), [0, 204, 0]);
    assert_eq!(pixel(&mut doc, 90, 50), [0, 204, 0]);
}

#[test]
fn scrolled_content_is_hit_inside_the_transform() {
    let doc = scrolled_scaled_scroller();
    let target = doc.get_element_by_id("target");
    assert_eq!(doc.element_from_point(30.0, 50.0), target);
    assert_eq!(doc.element_from_point(90.0, 50.0), target);
}
