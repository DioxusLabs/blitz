//! Headless test harness for Blitz documents.
//!
//! [`Harness`] wraps any [`blitz_dom::Document`] (e.g. [`blitz_html::HtmlDocument`] or
//! [`dioxus_native_dom::DioxusDocument`]) and provides:
//!
//! - Document construction helpers with sensible, deterministic defaults
//!   ([`Harness::from_html`], [`Harness::from_component`])
//! - A [`pump`](Harness::pump)/[`tick`](Harness::tick) loop that polls pending async work and
//!   resolves style/layout with a controlled animation clock
//! - DOM inspection helpers (selectors, layout rects, hit-testing, tree dumps)
//! - Programmatic input synthesis (clicks, taps, drags, wheel, keyboard, IME) that routes
//!   through the real event-dispatch pipeline, without requiring a window
//! - Deterministic network providers for fetching sub-resources from local fixture
//!   directories or record/replay caches ([`FileNetProvider`], [`RecordReplayProvider`])
//! - CPU rendering to RGBA buffers/PNGs ([`Harness::screenshot`]) and reference-image
//!   assertions with on-disk failure artifacts ([`Harness::assert_screenshot_matches`])
//!
//! No window, GPU, or compositor is required, so tests run headless.

mod harness;
mod input;
mod inspect;
mod net;
mod render;

pub use harness::{Harness, HarnessOptions};
pub use input::{key_event, mouse_pointer_event, pointer_event, touch_pointer_event};
pub use inspect::Rect;
pub use net::{
    FileNetProvider, RecordReplayMode, RecordReplayProvider, RequestCounts, load_data_url,
    load_fixture_bytes,
};
pub use render::{Screenshot, ScreenshotDiff, artifacts_dir, compare_screenshots};
