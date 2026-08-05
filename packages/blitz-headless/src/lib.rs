//! Headless document setup and rendering for Blitz.
//!
//! [`HeadlessDocument`] wraps any [`blitz_dom::Document`] (e.g.
//! [`blitz_html::HtmlDocument`]) and provides:
//!
//! - Document construction with configurable viewport, scale, color scheme, base url,
//!   and a pluggable [`NetProvider`](blitz_traits::net::NetProvider)
//!   ([`HeadlessDocument::from_html`], [`HeadlessDocument::from_html_with`])
//! - Style/layout resolution, including waiting for pending network requests to drain
//!   ([`HeadlessDocument::resolve_until_network_idle`])
//! - CPU rendering to RGBA buffers/PNGs ([`HeadlessDocument::screenshot`], [`Screenshot`])
//!
//! No window, GPU, or compositor is required.

mod document;
mod render;

pub use document::{HeadlessDocument, HeadlessOptions};
pub use render::{
    Screenshot, ScreenshotDiff, compare_screenshots, screenshot_document,
    screenshot_document_with_size,
};
