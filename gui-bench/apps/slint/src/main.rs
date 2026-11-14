//! Slint on its own event loop, timed with the window's rendering notifier: update = apply
//! the op to the Rust model and sync it into the `VecModel` (Slint's change notifications);
//! build = from the redraw request until `BeforeRendering` (property and layout
//! evaluation triggered by the redraw); raster_cpu = `BeforeRendering` to
//! `AfterRendering` (the renderer drawing the item tree, which also evaluates lazily
//! bound properties); present = `AfterRendering` until control returns to the event loop
//! (buffer swap).
//!
//! The renderer is Slint's default for the platform; set `SLINT_BACKEND` (e.g.
//! `winit-skia`, `winit-software`) to compare others. No `raster_gpu`: the rendering
//! notifier doesn't expose the renderer's command buffers.

use bench_core::scroll::BlockKind;
use bench_core::table::{Row, TableModel, TableOp};
use bench_core::{Args, Driver, Op, Phase, Workload};
use slint::{ComponentHandle, Model, ModelRc, RenderingState, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

slint::include_modules!();

struct State {
    driver: Driver,
    model: TableModel,
    rows: Rc<VecModel<RowData>>,
}

fn row_data(row: &Row, selected: Option<usize>) -> RowData {
    RowData {
        id: row.id as i32,
        label: SharedString::from(row.label.as_str()),
        selected: selected == Some(row.id),
    }
}

impl State {
    fn sync(&self, i: usize) {
        self.rows
            .set_row_data(i, row_data(&self.model.rows[i], self.model.selected));
    }

    fn apply(&mut self, op: TableOp) {
        let old_selected = self.model.selected;
        let old_len = self.model.rows.len();
        self.model.apply(op);
        let rows = &self.model.rows;
        match op {
            TableOp::Create(_) | TableOp::Clear => self
                .rows
                .set_vec(rows.iter().map(|r| row_data(r, None)).collect::<Vec<_>>()),
            TableOp::Append(_) => self.rows.extend(
                rows[old_len..]
                    .iter()
                    .map(|r| row_data(r, self.model.selected)),
            ),
            TableOp::UpdateEvery10th => (0..rows.len()).step_by(10).for_each(|i| self.sync(i)),
            TableOp::Select(_) => {
                for id in [old_selected, self.model.selected].into_iter().flatten() {
                    if let Some(i) = rows.iter().position(|r| r.id == id) {
                        self.sync(i);
                    }
                }
            }
            TableOp::SwapRows => {
                if rows.len() > 998 {
                    self.sync(1);
                    self.sync(998);
                }
            }
            TableOp::Remove(i) => {
                if i < old_len {
                    self.rows.remove(i);
                }
            }
        }
    }
}

fn next_frame(app: &App, state: &RefCell<State>) {
    let mut state = state.borrow_mut();
    if state.driver.in_frame() {
        state.driver.lap(Phase::Present);
        state.driver.end_frame();
    }
    let Some(op) = state.driver.begin_frame() else {
        let _ = slint::quit_event_loop();
        return;
    };
    match op {
        Op::Table(op) => state.apply(op),
        Op::Scroll { y } => app.set_scroll_y(y),
    }
    state.driver.lap(Phase::Update);
    app.window().request_redraw();
}

fn main() {
    let args = Args::parse();
    let app = App::new().unwrap();
    let rows = Rc::new(VecModel::<RowData>::default());
    app.set_rows(ModelRc::from(rows.clone()));
    if args.workload == Workload::Scroll {
        app.set_scroll_workload(true);
        let blocks: Vec<BlockData> = bench_core::scroll::document()
            .iter()
            .map(|b| BlockData {
                text: b.text.as_str().into(),
                kind: match b.kind {
                    BlockKind::Heading => 0,
                    BlockKind::Paragraph => 1,
                    BlockKind::Emphasis => 2,
                    BlockKind::Code => 3,
                },
            })
            .collect();
        app.set_blocks(ModelRc::new(VecModel::from(blocks)));
    }
    let state = Rc::new(RefCell::new(State {
        driver: Driver::new("slint", &args),
        model: TableModel::default(),
        rows,
    }));

    let weak = app.as_weak();
    let notifier_state = state.clone();
    app.window()
        .set_rendering_notifier(move |rendering, api| {
            let mut s = notifier_state.borrow_mut();
            match rendering {
                RenderingState::RenderingSetup => {
                    let api = match api {
                        slint::GraphicsAPI::NativeOpenGL { .. } => "OpenGL",
                        _ => "other",
                    };
                    let backend =
                        std::env::var("SLINT_BACKEND").unwrap_or_else(|_| "default".into());
                    s.driver.meta(&[
                        ("renderer", format!("slint {backend} backend ({api})")),
                        ("gpu_timing", "not exposed".into()),
                    ]);
                }
                RenderingState::BeforeRendering if s.driver.in_frame() => {
                    s.driver.lap(Phase::Build)
                }
                RenderingState::AfterRendering if s.driver.in_frame() => {
                    s.driver.lap(Phase::RasterCpu);
                    let (weak, state) = (weak.clone(), notifier_state.clone());
                    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                        if let Some(app) = weak.upgrade() {
                            next_frame(&app, &state);
                        }
                    });
                }
                _ => {}
            }
        })
        .expect("this Slint renderer has no rendering notifier");

    let (weak, start_state) = (app.as_weak(), state.clone());
    slint::Timer::single_shot(std::time::Duration::from_millis(100), move || {
        if let Some(app) = weak.upgrade() {
            next_frame(&app, &start_state);
        }
    });
    app.run().unwrap();
}
