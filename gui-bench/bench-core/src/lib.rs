//! Shared pieces of the gui-bench apps: the workloads (op scripts and their data), the
//! per-frame phase recorder, and a GPU timestamp helper that each app instantiates for
//! the wgpu version its toolkit uses.
//!
//! Every app runs the same scripted op sequence. Each op is applied at the start of a
//! frame, then the app renders that frame and reports how long each phase took:
//!
//! - `update`: applying the op to app state, plus the toolkit's reconcile/diff where it
//!   has one (Dioxus diffing, Iced `update` + `view` + widget tree diff).
//! - `build`: style, layout and building the display list / scene / primitives.
//! - `raster_cpu`: CPU time spent encoding and submitting GPU work.
//! - `raster_gpu`: GPU execution time, measured with GPU timestamp queries.
//! - `acquire` / `present`: getting the next swapchain image and presenting it.
//! - `total`: op injected -> present returned.
//!
//! Phases a toolkit doesn't expose are left out of its output rather than estimated.

pub mod args;
pub mod gpu_timer;
pub mod record;
pub mod scroll;
pub mod table;

pub use args::{Args, Workload};
pub use record::{Driver, Phase};

/// Logical window size every app uses.
pub const WINDOW_WIDTH: u32 = 1024;
pub const WINDOW_HEIGHT: u32 = 768;

/// One step of a workload script.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Table(table::TableOp),
    /// Scroll the document so `y` logical px are above the viewport.
    Scroll {
        y: f32,
    },
}
