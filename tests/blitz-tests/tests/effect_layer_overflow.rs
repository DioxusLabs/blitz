//! Opacity/filter layers must preserve visible descendants outside the border box.
use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

#[test]
fn effect_layers_preserve_visible_overflow_but_respect_hidden_overflow() {
    for scale in [0.5, 1.0, 2.0] {
        for (effect, pale) in [("opacity:.5", true), ("filter:blur(0px)", false)] {
            for overflow in ["visible", "hidden"] {
                let html = format!(
                    "<html><body style='margin:0;background:white'><div style='position:absolute;\
                     left:40px;top:40px;width:40px;height:40px;{effect};overflow:{overflow}'>\
                     <div style='position:absolute;left:-10px;top:-10px;width:60px;height:60px;\
                     background:blue'></div></div></body></html>"
                );
                let size = (120.0 * scale) as u32;
                let mut doc = HtmlDocument::from_html(
                    &html,
                    DocumentConfig {
                        viewport: Some(Viewport::new(size, size, scale, ColorScheme::Light)),
                        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
                        ..Default::default()
                    },
                );
                doc.resolve(0.0);
                let pixels = render_to_buffer::<VelloCpuImageRenderer, _>(
                    |scene| paint_scene(scene, doc.as_mut(), scale as f64, size, size, 0, 0),
                    size,
                    size,
                );
                for (x, y, outside) in [
                    (35.0, 60.0, true),
                    (85.0, 60.0, true),
                    (60.0, 35.0, true),
                    (60.0, 85.0, true),
                    (60.0, 60.0, false),
                ] {
                    let index = ((y * scale) as usize * size as usize + (x * scale) as usize) * 4;
                    let actual = &pixels[index..index + 3];
                    let expected = if outside && overflow == "hidden" {
                        [255; 3]
                    } else if pale {
                        [128, 128, 255]
                    } else {
                        [0, 0, 255]
                    };
                    assert!(
                        actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                        "{effect}, overflow:{overflow}, scale {scale}, ({x}, {y}): {actual:?} != {expected:?}"
                    );
                }
            }
        }
    }
}
