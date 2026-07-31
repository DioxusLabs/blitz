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
//!
//! No window, GPU, or compositor is required, so tests run headless.

mod harness;
mod input;
mod inspect;

pub use harness::{Harness, HarnessOptions};
pub use input::{key_event, mouse_pointer_event, pointer_event, touch_pointer_event};
pub use inspect::Rect;
