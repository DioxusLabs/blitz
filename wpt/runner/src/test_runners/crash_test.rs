use std::time::{Duration, Instant};

use anyrender::{ImageRenderer as _, PaintScene as _};
use blitz_dom::util::Color;
use blitz_dom::{BaseDocument, Document as _};
use blitz_paint::paint_scene;
use blitz_vibey_script::ScriptDocument;
use peniko::Fill;
use peniko::kurbo::Rect;

use super::harness_test::WptScriptFetcher;
use super::{parse_and_resolve_document, pump_net_provider};
use crate::{BufferKind, HEIGHT, SCALE, SubtestCounts, TestFlags, ThreadCtx, WIDTH};

/// How long to keep running pending JS timers (crashtests often crash in
/// `setTimeout`/`requestAnimationFrame` callbacks a few frames after load)
const TIMER_BUDGET: Duration = Duration::from_millis(100);

/// Runs a crashtest: the test passes if the document parses, executes its
/// scripts, resolves style/layout, and renders without panicking. No image
/// comparison is performed.
pub fn process_crash_test(
    ctx: &mut ThreadCtx,
    relative_path: &str,
    html: &str,
    flags: TestFlags,
) -> SubtestCounts {
    let mut document = parse_and_resolve_document(ctx, html, relative_path);

    if flags.contains(TestFlags::USES_SCRIPT) {
        let mut script_document = ScriptDocument::from_base_document(document)
            .with_fetcher(WptScriptFetcher::new(ctx.wpt_dir.clone()));
        script_document.execute_scripts();

        // Run JS timers due within a short budget, re-resolving in between
        // (crashes are often triggered by post-load DOM/style mutation)
        let deadline = Instant::now() + TIMER_BUDGET;
        while let Some(timer_deadline) = script_document.next_timer_deadline() {
            if timer_deadline > deadline {
                break;
            }
            let now = Instant::now();
            if timer_deadline > now {
                std::thread::sleep(timer_deadline - now);
            }
            script_document.poll(None);
            script_document.inner_mut().resolve(0.0);
        }

        // JS errors don't fail a crashtest, but drain them so they're not
        // misattributed elsewhere
        let _ = script_document.take_js_errors();

        let mut doc = script_document.inner_mut();
        doc.resolve(0.0);
        pump_net_provider(ctx, &mut doc);
        render_to_buffer(ctx, &mut doc);
    } else {
        render_to_buffer(ctx, &mut document);
    }

    SubtestCounts::ONE_OF_ONE
}

fn render_to_buffer(ctx: &mut ThreadCtx, document: &mut BaseDocument) {
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

            paint_scene(scene, document, SCALE, WIDTH, HEIGHT, 0, 0);
        },
        buf,
    );
}
