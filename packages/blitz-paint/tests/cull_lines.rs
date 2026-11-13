//! A paragraph that runs past the viewport paints only the lines near it.
//!
//! WPT can't see this (the pixels are the same either way), so the glyph runs are inspected in
//! a recorded scene instead.

use anyrender::{Scene, recording::RenderCommand};
use blitz_dom::{Attribute, BaseDocument, DocumentConfig, qual_name};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use kurbo::Point;

const WIDTH: u32 = 400;
const HEIGHT: u32 = 300;
const LINE_HEIGHT: f64 = 30.0;

/// Paints a ~230-line paragraph scrolled to `scroll_y` at `scale`, and returns the
/// baseline of every glyph run, in viewport pixels.
fn painted_baselines(scroll_y: f64, scale: f64) -> Vec<f64> {
    let (width, height) = (
        (WIDTH as f64 * scale) as u32,
        (HEIGHT as f64 * scale) as u32,
    );
    let mut doc = BaseDocument::new(DocumentConfig {
        viewport: Some(Viewport::new(
            width,
            height,
            scale as f32,
            ColorScheme::Light,
        )),
        ..Default::default()
    });

    // The paragraph is in the bullet font every document registers, so no system
    // fonts are needed.
    let style = |value: &str| {
        vec![Attribute {
            name: qual_name!("style"),
            value: value.to_string(),
        }]
    };
    let root_id = doc.root_node().id;
    let mut mutator = doc.mutate();
    let html = mutator.create_element(qual_name!("html", html), vec![]);
    let body = mutator.create_element(qual_name!("body", html), style("margin:0"));
    let p = mutator.create_element(
        qual_name!("p", html),
        style(&format!("margin:0;font:20px/{LINE_HEIGHT}px Bullet")),
    );
    let text = mutator.create_text_node(&"\u{2022}\u{2022} ".repeat(3000));
    mutator.append_children(p, &[text]);
    mutator.append_children(body, &[p]);
    mutator.append_children(html, &[body]);
    mutator.append_children(root_id, &[html]);
    drop(mutator);

    doc.resolve(0.0);
    doc.set_viewport_scroll(blitz_dom::Point {
        x: 0.0,
        y: scroll_y,
    });
    doc.resolve(0.0);

    // Paint at an offset, as a sub-document would be.
    let (x_offset, y_offset) = (50, 70);
    let mut scene = Scene::new();
    paint_scene(
        &mut scene, &mut doc, scale, width, height, x_offset, y_offset,
    );

    scene
        .commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::GlyphRun(run) => run.glyphs.first().map(|glyph| {
                let y = (run.transform * Point::new(glyph.x as f64, glyph.y as f64)).y;
                (y - y_offset as f64) / scale
            }),
            _ => None,
        })
        .collect()
}

#[test]
fn lines_outside_the_viewport_are_not_painted() {
    for scale in [1.0, 2.0] {
        let baselines = painted_baselines(4000.5, scale);
        assert!(!baselines.is_empty(), "no text painted at scale {scale}");

        // Lines are kept within one line height of slack of the viewport, so a kept line's
        // baseline is at most two line heights beyond either edge.
        let (min, max) = baselines
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), &y| (lo.min(y), hi.max(y)));
        assert!(
            min >= -2.0 * LINE_HEIGHT && max <= HEIGHT as f64 + 2.0 * LINE_HEIGHT,
            "painted lines from {min} to {max} at scale {scale}, viewport is 0 to {HEIGHT}"
        );

        // ...and the lines crossing the viewport's edges are still painted.
        assert!(
            min < 0.0 && max > HEIGHT as f64,
            "lines crossing the viewport edge were culled at scale {scale}: {min} to {max}"
        );
    }
}
