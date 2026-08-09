use anyrender::{ImageRenderer as _, PaintScene as _};
use blitz_dom::util::Color;
use blitz_paint::paint_scene;
use peniko::Fill;
use peniko::kurbo::Rect;

use super::parse_and_resolve_document;
use crate::{BufferKind, HEIGHT, SCALE, SubtestCounts, ThreadCtx, WIDTH};

/// Runs a crashtest: the test passes if the document parses, resolves style/layout,
/// and renders without panicking. No image comparison is performed.
pub fn process_crash_test(ctx: &mut ThreadCtx, relative_path: &str, html: &str) -> SubtestCounts {
    let mut document = parse_and_resolve_document(ctx, html, relative_path);

    let buf = ctx.buffers.get_mut(BufferKind::Test);
    ctx.renderer.render_to_vec(
        |scene| {
            scene.reset();

            // Render white background
            scene.fill(
                Fill::NonZero,
                Default::default(),
                Color::WHITE,
                Default::default(),
                &Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
            );

            paint_scene(scene, &mut document, SCALE, WIDTH, HEIGHT, 0, 0);
        },
        buf,
    );

    SubtestCounts::ONE_OF_ONE
}
