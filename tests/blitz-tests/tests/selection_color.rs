//! Selected text is highlighted with the background color of `::selection`,
//! and with the default highlight when no rule sets one.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const W: usize = 200;
const H: usize = 40;

/// The default highlight (`SELECTION_COLOR`).
const DEFAULT: [u8; 3] = [180, 213, 255];

/// Selects all the text of a paragraph styled by `css` and returns a pixel of
/// its first line above the glyphs, where only the highlight is painted.
fn highlight(css: &str) -> [u8; 3] {
    let html = format!(
        "<html><head><style>{css}</style></head>\
         <body style='margin:0; background:#fff'>\
         <p id='text' style='margin:0; font-size:20px; line-height:40px; color:#fff'>MMMM</p>\
         </body></html>"
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(W as u32, H as u32, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let text = doc.query_selector("#text").unwrap().unwrap();
    doc.set_text_selection(text, 0, text, 4);

    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, W as u32, H as u32, 0, 0),
        W as u32,
        H as u32,
    );
    let (x, y) = (4, 3);
    let i = (y * W + x) * 4;
    [buffer[i], buffer[i + 1], buffer[i + 2]]
}

fn assert_near(actual: [u8; 3], expected: [u8; 3]) {
    let close = actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2);
    assert!(close, "expected {expected:?}, got {actual:?}");
}

#[test]
fn selection_is_highlighted_with_the_selection_background_color() {
    assert_near(
        highlight("::selection { background-color: rgb(200, 30, 60) }"),
        [200, 30, 60],
    );
}

#[test]
fn selection_without_a_rule_uses_the_default_highlight() {
    assert_near(highlight(""), DEFAULT);
}

#[test]
fn selection_rule_without_a_background_keeps_the_default_highlight() {
    assert_near(highlight("::selection { color: rgb(0, 0, 0) }"), DEFAULT);
}
