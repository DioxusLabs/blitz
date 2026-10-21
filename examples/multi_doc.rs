//! Resolves several `BaseDocument`s with `StyleThreading::Parallel` in the
//! ways an embedder might: alternating on one thread, nested (a sub-document
//! resolved from its parent's `resolve`), and concurrently from many threads.
//!
//! Usage: `multi_doc <interleaved|nested|threads> [iterations]`
//!
//! Environment: `MULTI_DOC_SEQ=1` uses `StyleThreading::Sequential` instead,
//! `MULTI_DOC_N=<n>` sets the number of top-level elements (default 2000),
//! `MULTI_DOC_FLAT=1` makes them leaves instead of `<div><p/><span/></div>`.
//! Build with `--features blitz-dom/log-phase-times` to see per-phase times.

use std::time::{Duration, Instant};

use blitz_dom::{BaseDocument, Document as _, DocumentConfig, StyleThreading};
use blitz_html::HtmlDocument;

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty())
}

fn html(n: usize) -> String {
    let flat = env_flag("MULTI_DOC_FLAT");
    let mut s = String::from("<style>.a{color:red}.b{display:flex}.c p{margin:1px}</style><body>");
    for i in 0..n {
        let class = ["a", "b", "c"][i % 3];
        let children = if flat { "" } else { "<p>x</p><span>y</span>" };
        s.push_str(&format!(
            "<div class=\"{class}\" style=\"width:{i}px\">{children}</div>"
        ));
    }
    s.push_str("</body>");
    s
}

fn config() -> DocumentConfig {
    let style_threading = if env_flag("MULTI_DOC_SEQ") {
        StyleThreading::Sequential
    } else {
        StyleThreading::Parallel
    };
    DocumentConfig {
        style_threading,
        ..Default::default()
    }
}

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "interleaved".into());
    let iters: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let n: usize = std::env::var("MULTI_DOC_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000);
    let html = html(n);

    let start = Instant::now();
    let mut resolve_time = Duration::ZERO;
    let mut timed_resolve = |doc: &mut BaseDocument| {
        let t = Instant::now();
        doc.resolve(0.0);
        resolve_time += t.elapsed();
    };

    match mode.as_str() {
        // Two documents resolved alternately on one thread.
        "interleaved" => {
            for _ in 0..iters {
                let mut a = HtmlDocument::from_html(&html, config());
                let mut b = HtmlDocument::from_html(&html, config());
                timed_resolve(&mut a.inner_mut());
                timed_resolve(&mut b.inner_mut());
                timed_resolve(&mut a.inner_mut());
            }
        }
        // Document B nested inside document A, resolved from within A's resolve.
        "nested" => {
            for _ in 0..iters {
                let mut a = HtmlDocument::from_html(&html, config()).into_inner();
                let b = HtmlDocument::from_html(&html, config()).into_inner();
                let host = a.root_element().id;
                a.set_sub_document(host, Box::new(b));
                timed_resolve(&mut a);
            }
        }
        // Many threads each resolving their own documents concurrently (issue #430).
        "threads" => {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let html = html.clone();
                    std::thread::spawn(move || {
                        for _ in 0..iters {
                            HtmlDocument::from_html(&html, config())
                                .inner_mut()
                                .resolve(0.0);
                        }
                    })
                })
                .collect();
            for handle in handles {
                handle.join().expect("a worker thread panicked");
            }
        }
        other => panic!("unknown mode {other}"),
    }

    println!(
        "{mode}: ok in {:?} (resolve: {:?})",
        start.elapsed(),
        resolve_time
    );
}
