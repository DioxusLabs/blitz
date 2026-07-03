//! Render culling must account for the pinch-zoom scale and offset: content whose
//! origin is outside the visual viewport but which extends into it must not be culled.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

#[test]
fn zoomed_content_extending_into_view_is_not_culled() {
    let mut doc = HtmlDocument::from_html(
        r#"<html><body style="margin:0">
            <div style="width:100px; height:100px; background:#ff0000;"></div>
        </body></html>"#,
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            pinch_zoom_enabled: Some(true),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    // Zoom in 4x anchored at the bottom-right corner: the pinch-zoom offset becomes
    // (75, 75), so the div's origin is well outside of the visual viewport while the
    // div itself still covers all of it.
    doc.pinch_zoom_by(4.0, 100.0, 100.0);
    doc.resolve(0.0);
    assert_eq!(doc.pinch_zoom().scale, 4.0);
    assert_eq!(
        doc.pinch_zoom().offset,
        blitz_dom::Point { x: 75.0, y: 75.0 }
    );

    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let idx = (50 * 100 + 50) * 4;
    assert_eq!(
        [buffer[idx], buffer[idx + 1], buffer[idx + 2]],
        [255, 0, 0],
        "zoomed content extending into the visual viewport must not be culled"
    );
}
