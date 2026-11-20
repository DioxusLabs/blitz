//! Text directly inside a scroll container moves by the scroll offset once,
//! the same as its block children.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{DocumentConfig, ScrollBehavior};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 100;

fn pixel(doc: &mut HtmlDocument, x: usize, y: usize) -> [u8; 3] {
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc, 1.0, WIDTH, HEIGHT, 0, 0),
        WIDTH,
        HEIGHT,
    );
    let idx = (y * WIDTH as usize + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

#[test]
fn own_inline_content_scrolls_once() {
    let mut doc = HtmlDocument::from_html(
        r#"<html><body style="margin:0;background:white">
        <div id=scroller style="position:absolute;left:20px;top:20px;width:40px;height:40px;
            overflow:hidden;background:red;font:10px/20px monospace;white-space:nowrap"><span
            id=first style="color:#0000ff;background:#0000ff">AAAAAAAAAA</span><span
            id=second style="color:#00cc00;background:#00cc00">BBBBBBBBBB</span></div>
        </body></html>"#,
        DocumentConfig {
            viewport: Some(Viewport::new(WIDTH, HEIGHT, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let scroller = doc.get_element_by_id("scroller").unwrap();
    let second = doc.get_element_by_id("second").unwrap();
    let edge = doc.get_client_bounding_rect(second).unwrap().x;

    doc.scroll_to(scroller, 40.0, 0.0, ScrollBehavior::Instant);
    assert_eq!(doc.get_node(scroller).unwrap().scroll_offset().x, 40.0);

    // The boundary between the two spans moves left by the 40px scroll offset.
    let edge = (edge - 40.0) as usize;
    assert!((25..55).contains(&edge), "{edge}");
    assert_eq!(pixel(&mut doc, edge - 3, 30), [0, 0, 255]);
    assert_eq!(pixel(&mut doc, edge + 3, 30), [0, 204, 0]);
    assert_eq!(
        doc.element_from_point((edge + 3) as f32, 30.0),
        Some(second)
    );
}
