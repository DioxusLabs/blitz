//! Headless benchmark: parse + style + layout of a local HTML file (e.g. a saved copy of
//! <https://en.wikipedia.org/wiki/Barack_Obama> with its stylesheets rewritten to local files).
//!
//! Usage: cargo run --release --example obama_bench -- <path/to/page.html> [iterations] [warmups]
//!
//! Prints one JSON line with median/p90 wall time for (i) a fresh parse+style+layout and
//! (ii) steady-state relayout of the same document, plus heap usage from a counting allocator.
//! Only `file://` URLs are fetched; the network is never touched.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};

struct Counting;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let cur = ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(cur, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOCATED.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                let cur = ALLOCATED.fetch_add(new_size - layout.size(), Ordering::Relaxed)
                    + new_size
                    - layout.size();
                PEAK.fetch_max(cur, Ordering::Relaxed);
            } else {
                ALLOCATED.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Serves `file://` URLs synchronously from disk; ignores everything else (no network).
struct LocalFiles;

impl NetProvider for LocalFiles {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        if request.url.scheme() != "file" {
            return;
        }
        let Ok(path) = request.url.to_file_path() else {
            return;
        };
        // Missing assets (images, etc. not saved alongside the page) are silently skipped.
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        handler.bytes(request.url.to_string(), Bytes::from(bytes));
    }
}

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;

fn viewport(scale: f32) -> Viewport {
    Viewport::new(WIDTH, HEIGHT, scale, ColorScheme::Light)
}

fn build(html: &str, base_url: &str, net: &Arc<LocalFiles>) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            base_url: Some(base_url.to_string()),
            net_provider: Some(Arc::clone(net) as _),
            viewport: Some(viewport(1.0)),
            ..Default::default()
        },
    );
    // Stylesheets are delivered synchronously by `LocalFiles`; two resolves make sure
    // any nested `@import`s delivered during the first are applied too.
    doc.resolve(0.0);
    doc.resolve(0.0);
    doc
}

fn stats(mut v: Vec<f64>) -> (f64, f64) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = v[v.len() / 2];
    let p90 = v[((v.len() as f64 * 0.9).ceil() as usize).min(v.len()) - 1];
    (median, p90)
}

fn main() {
    let path = std::env::args().nth(1).expect("html path");
    let iterations: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let warmups: usize = std::env::args()
        .nth(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);

    let path = std::fs::canonicalize(&path).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    let base_url = format!("file://{}", path.display());
    let net = Arc::new(LocalFiles);

    let heap_before = ALLOCATED.load(Ordering::Relaxed);

    // (i) full parse + style + layout, repeated from scratch
    let mut full = Vec::new();
    let mut doc = None;
    let mut heap_after_first_layout = 0;
    let mut root_height = 0.0;
    for i in 0..warmups + iterations {
        let start = Instant::now();
        let d = build(&html, &base_url, &net);
        let elapsed = start.elapsed().as_secs_f64() * 1e3;
        if i == 0 {
            heap_after_first_layout = ALLOCATED.load(Ordering::Relaxed) - heap_before;
            root_height = d.as_ref().root_element().final_layout().size.height;
        }
        if i >= warmups {
            full.push(elapsed);
        }
        doc = Some(d);
    }
    let mut doc = doc.unwrap();
    let heap_after_layout = ALLOCATED.load(Ordering::Relaxed) - heap_before;

    // (ii) steady-state relayout: toggling the scale invalidates every inline (text)
    // layout, rebuilds the stylist device (restyle) and re-runs layout.
    let scales = [1.0_f32, 1.00001_f32];
    let mut relayout = Vec::new();
    for i in 0..warmups + iterations {
        doc.as_mut().set_viewport(viewport(scales[i % 2]));
        let start = Instant::now();
        doc.resolve(0.0);
        let elapsed = start.elapsed().as_secs_f64() * 1e3;
        if i >= warmups {
            relayout.push(elapsed);
        }
    }

    let (full_med, full_p90) = stats(full);
    let (re_med, re_p90) = stats(relayout);

    println!(
        "{{\"first_layout_ms\": {{\"median\": {full_med:.2}, \"p90\": {full_p90:.2}}}, \
         \"relayout_ms\": {{\"median\": {re_med:.2}, \"p90\": {re_p90:.2}}}, \
         \"heap_after_first_layout_mb\": {:.2}, \"heap_after_layout_mb\": {:.2}, \
         \"heap_peak_mb\": {:.2}, \"root_height\": {root_height:.1}}}",
        heap_after_first_layout as f64 / 1e6,
        heap_after_layout as f64 / 1e6,
        PEAK.load(Ordering::Relaxed) as f64 / 1e6,
    );
}
