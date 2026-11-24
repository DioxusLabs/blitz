//! The text an input method is composing in a text input is underlined until it is committed.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_paint::paint_scene;
use blitz_test_harness::Harness;
use blitz_traits::events::BlitzImeEvent;

const HTML: &str = r#"<html><body style="margin:0; background:#fff">
    <input id="input" type="text" style="width:300px; height:80px; border:0; outline:0;
        padding:0; background:#fff; color:#f00; caret-color:transparent; font-size:40px">
</body></html>"#;

/// How many pixels are painted in the text's colour.
fn red_pixels(harness: &mut Harness) -> usize {
    harness.pump();
    let mut doc = harness.base_mut();
    let (width, height) = (800, 600);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, width, height, 0, 0),
        width,
        height,
    );
    buffer
        .chunks_exact(4)
        .filter(|px| px[0] > 200 && px[1] < 50 && px[2] < 50)
        .count()
}

#[test]
fn composing_text_is_underlined_until_committed() {
    let mut harness = Harness::from_html(HTML);
    harness.click("#input");
    assert_eq!(red_pixels(&mut harness), 0);

    // Spaces have no ink, so the only thing painted in the text's colour is the underline.
    harness.ime(BlitzImeEvent::Preedit("    ".into(), Some((4, 4))));
    assert!(red_pixels(&mut harness) > 0);

    harness.ime(BlitzImeEvent::Commit("    ".into()));
    assert_eq!(red_pixels(&mut harness), 0);
}
