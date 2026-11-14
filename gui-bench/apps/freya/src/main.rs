//! Freya on its own runner, timed with Freya's plugin events: update = the op is sent to
//! the app, applied to `State`, and Freya re-runs components, diffs and applies the
//! mutations to its tree (until `StartedMeasuringLayout`); build = Torin layout
//! (`Started/FinishedMeasuringLayout`) and the render pipeline issuing Skia draw calls
//! (`BeforeRender`..`AfterRender`); raster_cpu = `BeforePresenting`..`AfterPresenting`:
//! Skia flush, GPU submission and present, so there is no separate present.
//!
//! No `raster_gpu`: Skia owns its command buffers. Freya's GL driver also sleeps to the
//! display refresh after presenting (`wait_for_display`), which lands in raster_cpu.

use bench_core::scroll::{Block, BlockKind};
use bench_core::table::TableModel;
use bench_core::{Args, Driver, Op, Phase, Workload};
use freya::prelude::*;
use freya_winit::plugins::{FreyaPlugin, PluginEvent, PluginHandle};
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use std::cell::{Cell, RefCell};

#[cfg(target_os = "macos")]
const MONOSPACE: &str = "Menlo";
#[cfg(not(target_os = "macos"))]
const MONOSPACE: &str = "DejaVu Sans Mono";

thread_local! {
    static WORKLOAD: Cell<Workload> = const { Cell::new(Workload::Table) };
    static OPS: RefCell<Option<UnboundedReceiver<Op>>> = const { RefCell::new(None) };
}

struct BenchPlugin {
    driver: Driver,
    ops: UnboundedSender<Op>,
}

impl BenchPlugin {
    fn next(&mut self) {
        if self.driver.in_frame() {
            self.driver.end_frame();
        }
        match self.driver.begin_frame() {
            Some(op) => self.ops.unbounded_send(op).unwrap(),
            None => std::process::exit(0),
        }
    }
}

impl FreyaPlugin for BenchPlugin {
    fn plugin_id(&self) -> &'static str {
        "gui-bench"
    }

    fn on_event(&mut self, event: &mut PluginEvent, _handle: PluginHandle) {
        let d = &mut self.driver;
        match event {
            PluginEvent::WindowCreated {
                graphics_driver,
                gpu_name,
                window,
                ..
            } => {
                d.meta(&[
                    ("renderer", format!("freya skia ({graphics_driver})")),
                    ("adapter", gpu_name.unwrap_or("-").to_string()),
                    ("gpu_timing", "not exposed".into()),
                    ("scale_factor", window.scale_factor().to_string()),
                ]);
            }
            _ if !d.in_frame() => {}
            PluginEvent::StartedMeasuringLayout { .. } => d.lap(Phase::Update),
            PluginEvent::FinishedMeasuringLayout { .. }
            | PluginEvent::AfterRender { .. }
            | PluginEvent::BeforePresenting { .. } => d.lap(Phase::Build),
            PluginEvent::BeforeRender { .. } => d.skip(),
            PluginEvent::AfterPresenting { .. } => {
                d.lap(Phase::RasterCpu);
                d.presented();
            }
            _ => {}
        }
        if matches!(
            event,
            PluginEvent::WindowCreated { .. } | PluginEvent::AfterRedraw { .. }
        ) && (!self.driver.in_frame() || matches!(event, PluginEvent::AfterRedraw { .. }))
        {
            self.next();
        }
    }
}

fn table(model: &TableModel) -> Element {
    rect()
        .children(model.rows.iter().map(|row| {
            let colour = if model.selected == Some(row.id) {
                (204, 0, 0)
            } else {
                (0, 0, 0)
            };
            rect()
                .key(row.id)
                .horizontal()
                .width(Size::fill())
                .content(Content::Flex)
                .height(Size::px(24.))
                .spacing(8.)
                .child(label().width(Size::px(60.)).text(row.id.to_string()))
                .child(
                    label()
                        .width(Size::flex(1.))
                        .color(colour)
                        .text(row.label.clone()),
                )
                .child(Button::new().child("x"))
        }))
        .into()
}

fn document(doc: &[Block]) -> Element {
    rect()
        .padding(8.)
        .spacing(8.)
        .children(doc.iter().map(|block| {
            let text = label()
                .text(block.text.clone())
                .font_size(block.kind.font_size());
            match block.kind {
                BlockKind::Heading => text.font_weight(FontWeight::BOLD),
                BlockKind::Paragraph => text,
                BlockKind::Emphasis => text.font_slant(FontSlant::Italic),
                BlockKind::Code => text.font_family(MONOSPACE),
            }
        }))
        .into()
}

fn app() -> impl IntoElement {
    use_init_theme(light_theme);
    let mut model = use_state(TableModel::default);
    let mut tick = use_state(|| 0u64);
    let doc = use_state(bench_core::scroll::document);
    let mut scroll = use_scroll_controller(ScrollConfig::default);
    use_hook(move || {
        let mut ops = OPS.with_borrow_mut(Option::take).expect("ops channel");
        spawn(async move {
            while let Some(op) = ops.next().await {
                match op {
                    Op::Table(op) => model.write().apply(op),
                    Op::Scroll { y } => {
                        scroll.scroll_to_y(-(y as i32));
                    }
                }
                // Every op must produce a frame, even one that changes nothing visible.
                *tick.write() += 1;
            }
        });
    });
    let content = match WORKLOAD.get() {
        Workload::Table => table(&model.read()),
        Workload::Scroll => document(&doc.read()),
    };
    rect()
        .expanded()
        .theme_background()
        .theme_color()
        .child(label().text(tick.read().to_string()).font_size(1.))
        .child(ScrollView::new_controlled(scroll).child(content))
}

fn main() {
    let args = Args::parse();
    WORKLOAD.set(args.workload);
    let (tx, rx) = unbounded();
    OPS.set(Some(rx));
    let plugin = BenchPlugin {
        driver: Driver::new("freya", &args),
        ops: tx,
    };
    launch(
        LaunchConfig::new()
            .with_window(
                WindowConfig::new(app)
                    .with_size(
                        bench_core::WINDOW_WIDTH as f64,
                        bench_core::WINDOW_HEIGHT as f64,
                    )
                    .with_title("gui-bench: freya")
                    .with_resizable(false),
            )
            .with_plugin(plugin),
    )
}
