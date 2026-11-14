//! GPUI on its own platform loop, timed from inside the frame: update = apply the op and
//! `notify` the view, from an `on_next_frame` callback so it runs at the start of the
//! frame GPUI draws it in; build = `render` + Taffy layout + prepaint + paint into the
//! scene (ending in a `canvas` painted after everything else); raster_cpu = from the end
//! of paint until a foreground task scheduled from paint runs: the platform renderer
//! (Metal on macOS, Blade on Linux) drawing the scene and presenting it, so there is no
//! separate present.
//!
//! GPUI re-renders a notified view's whole element tree each frame (no keyed diff); and
//! frames are driven by the display link, so GPUI runs at the display refresh rate.
//! No `raster_gpu`: the platform renderer's command buffers aren't exposed.

use bench_core::scroll::{Block, BlockKind};
use bench_core::table::TableModel;
use bench_core::{Args, Driver, Op, Phase, Workload};
use gpui::{
    App, Application, Bounds, Context, FontWeight, Render, ScrollHandle, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, canvas, div, point, prelude::*, px, rgb, size,
};
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(target_os = "macos")]
const MONOSPACE: &str = "Menlo";
#[cfg(not(target_os = "macos"))]
const MONOSPACE: &str = "DejaVu Sans Mono";

struct Bench {
    workload: Workload,
    driver: Rc<RefCell<Driver>>,
    model: TableModel,
    doc: Vec<Block>,
    scroll: ScrollHandle,
}

impl Render for Bench {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.workload {
            Workload::Table => div().id("table").size_full().overflow_y_scroll().children(
                self.model.rows.iter().map(|row| {
                    let colour = if self.model.selected == Some(row.id) {
                        rgb(0xcc0000)
                    } else {
                        rgb(0x000000)
                    };
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .h(px(24.))
                        .child(div().w(px(60.)).child(row.id.to_string()))
                        .child(div().flex_1().text_color(colour).child(row.label.clone()))
                        .child(
                            div()
                                .px_2()
                                .border_1()
                                .border_color(rgb(0x888888))
                                .child("x"),
                        )
                }),
            ),
            Workload::Scroll => div()
                .id("document")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .p_2()
                .flex()
                .flex_col()
                .gap_2()
                .children(self.doc.iter().map(|block| {
                    let text = div()
                        .text_size(px(block.kind.font_size()))
                        .child(block.text.clone());
                    match block.kind {
                        BlockKind::Heading => text.font_weight(FontWeight::BOLD),
                        BlockKind::Paragraph => text,
                        BlockKind::Emphasis => text.italic(),
                        BlockKind::Code => text.font_family(MONOSPACE),
                    }
                })),
        };
        let driver = self.driver.clone();
        let handle = cx.entity();
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x000000))
            .text_size(px(16.))
            .child(content)
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, cx| {
                        let mut d = driver.borrow_mut();
                        if !d.in_frame() {
                            return;
                        }
                        d.lap(Phase::Build);
                        let driver = driver.clone();
                        let window_handle = window.window_handle();
                        cx.spawn(async move |cx| {
                            {
                                let mut d = driver.borrow_mut();
                                d.lap(Phase::RasterCpu);
                                d.end_frame();
                            }
                            let _ = cx.update_window(window_handle, |_, window, _| {
                                schedule_next(window, handle);
                            });
                        })
                        .detach();
                    },
                )
                .absolute()
                .size_0(),
            )
    }
}

fn schedule_next(window: &mut Window, view: gpui::Entity<Bench>) {
    window.on_next_frame(move |window, cx| {
        view.update(cx, |bench, cx| {
            let op = bench.driver.borrow_mut().begin_frame();
            let Some(op) = op else {
                cx.quit();
                return;
            };
            match op {
                Op::Table(op) => bench.model.apply(op),
                Op::Scroll { y } => bench.scroll.set_offset(point(px(0.), px(-y))),
            }
            cx.notify();
            bench.driver.borrow_mut().lap(Phase::Update);
        });
        window.refresh();
    });
    window.refresh();
}

fn main() {
    let args = Args::parse();
    let driver = Rc::new(RefCell::new(Driver::new("gpui", &args)));
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(
            None,
            size(
                px(bench_core::WINDOW_WIDTH as f32),
                px(bench_core::WINDOW_HEIGHT as f32),
            ),
            cx,
        );
        let renderer = if cfg!(target_os = "macos") {
            "gpui (Metal)"
        } else {
            "gpui (Blade)"
        };
        driver.borrow_mut().meta(&[
            ("renderer", renderer.into()),
            ("gpu_timing", "not exposed".into()),
        ]);
        let workload = args.workload;
        let window: WindowHandle<Bench> = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    is_resizable: false,
                    titlebar: Some(TitlebarOptions {
                        title: Some("gui-bench: gpui".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| Bench {
                        workload,
                        driver: driver.clone(),
                        model: TableModel::default(),
                        doc: bench_core::scroll::document(),
                        scroll: ScrollHandle::new(),
                    })
                },
            )
            .unwrap();
        window
            .update(cx, |_, window, cx| schedule_next(window, cx.entity()))
            .unwrap();
        cx.activate(true);
    });
}
