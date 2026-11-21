//! Dioxus native (Blitz) on its own winit loop, so each phase can be timed:
//! update = apply the op, mark the root component dirty and `poll` the document (Dioxus
//! re-renders and diffs, and the mutations are applied to the Blitz DOM); build = Stylo
//! style resolution + Taffy layout + Parley text layout (`resolve`) and `paint_scene`
//! building the Vello scene; raster_cpu = anyrender_vello's `render` after the scene is
//! built: Vello encoding + GPU submission, swapchain acquire, blit + present, and waiting
//! for the GPU (anyrender_vello waits for the GPU every frame), so there is no separate
//! acquire/present.
//!
//! No `raster_gpu`: Vello creates and submits its own command encoders.

use anyrender::WindowRenderer;
use anyrender_vello::VelloWindowRenderer;
use bench_core::scroll::{Block, BlockKind};
use bench_core::table::TableModel;
use bench_core::{Args, Driver, Op, Phase, Workload};
use blitz_dom::{Document, DocumentConfig};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus_native::prelude::*;
use dioxus_native::{DioxusDocument, winit};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

const CSS: &str = r#"
body { margin: 0; font-family: sans-serif; font-size: 16px; }
.row { display: flex; gap: 8px; height: 24px; align-items: center; }
.row .id { width: 60px; }
.row .label { flex: 1; }
.row.selected .label { color: #c00; }
.doc { padding: 8px; }
.doc p { margin: 0 0 8px 0; }
.doc .heading { font-size: 24px; font-weight: bold; }
.doc .emphasis { font-style: italic; }
.doc .code { font-family: monospace; font-size: 14px; }
"#;

thread_local! {
    static WORKLOAD: Cell<Workload> = const { Cell::new(Workload::Table) };
    static MODEL: RefCell<TableModel> = RefCell::new(TableModel::default());
    static DOC: Vec<Block> = bench_core::scroll::document();
}

fn app() -> Element {
    match WORKLOAD.get() {
        Workload::Table => MODEL.with_borrow(|model| {
            rsx! {
                style { {CSS} }
                div {
                    for row in model.rows.iter() {
                        div {
                            key: "{row.id}",
                            class: if model.selected == Some(row.id) { "row selected" } else { "row" },
                            span { class: "id", "{row.id}" }
                            span { class: "label", "{row.label}" }
                            button { "x" }
                        }
                    }
                }
            }
        }),
        Workload::Scroll => DOC.with(|doc| {
            rsx! {
                style { {CSS} }
                div { class: "doc",
                    for block in doc.iter() {
                        p {
                            class: match block.kind {
                                BlockKind::Heading => "heading",
                                BlockKind::Paragraph => "paragraph",
                                BlockKind::Emphasis => "emphasis",
                                BlockKind::Code => "code",
                            },
                            "{block.text}"
                        }
                    }
                }
            }
        }),
    }
}

struct App {
    driver: Driver,
    doc: DioxusDocument,
    window: Option<Arc<dyn Window>>,
    renderer: VelloWindowRenderer,
}

impl App {
    fn frame(&mut self, event_loop: &dyn ActiveEventLoop) {
        let Some(op) = self.driver.begin_frame() else {
            event_loop.exit();
            return;
        };
        let mut scroll_y = None;
        match op {
            Op::Table(op) => {
                MODEL.with_borrow_mut(|model| model.apply(op));
                self.doc.vdom.mark_dirty(ScopeId::APP);
            }
            Op::Scroll { y } => scroll_y = Some(y as f64),
        }
        self.doc.poll(None);
        self.driver.lap(Phase::Update);

        let mut inner = self.doc.inner_mut();
        if let Some(y) = scroll_y {
            let current = inner.viewport_scroll().y;
            inner.scroll_viewport_by(0.0, y - current);
        }
        inner.resolve(0.0);
        let (width, height) = inner.viewport().window_size;
        let scale = inner.viewport().scale_f64();
        self.driver.lap(Phase::Build);
        let driver = &mut self.driver;
        self.renderer.render(|scene| {
            paint_scene(scene, &mut inner, scale, width, height, 0, 0);
            driver.lap(Phase::Build);
        });
        drop(inner);
        self.driver.lap(Phase::RasterCpu);
        self.driver.end_frame();
        self.window.as_ref().unwrap().request_redraw();
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("gui-bench: dioxus-native")
            .with_resizable(false)
            .with_surface_size(winit::dpi::LogicalSize::new(
                bench_core::WINDOW_WIDTH,
                bench_core::WINDOW_HEIGHT,
            ));
        let window: Arc<dyn Window> = Arc::from(event_loop.create_window(attrs).unwrap());
        let size = window.surface_size();
        let scale = window.scale_factor() as f32;
        self.doc.inner_mut().set_viewport(Viewport::new(
            size.width,
            size.height,
            scale,
            ColorScheme::Light,
        ));
        self.renderer
            .resume(Arc::new(window.clone()), size.width, size.height, || {});
        assert!(self.renderer.complete_resume());
        self.driver.meta(&[
            ("renderer", "anyrender_vello (vello)".into()),
            ("gpu_timing", "not exposed".into()),
            ("scale_factor", scale.to_string()),
        ]);
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }
}

fn main() {
    let args = Args::parse();
    WORKLOAD.set(args.workload);
    let driver = Driver::new("dioxus-native", &args);
    let vdom = VirtualDom::new(app);
    let mut doc = DioxusDocument::new(vdom, DocumentConfig::default());
    doc.initial_build();
    let app = App {
        driver,
        doc,
        window: None,
        renderer: VelloWindowRenderer::new(),
    };
    EventLoop::new().unwrap().run_app(app).unwrap();
}
