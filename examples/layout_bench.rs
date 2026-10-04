//! Time the layout of a web page: load the first CLI argument as a URL (or a path to an HTML file),
//! then repeatedly lay the document out from scratch.
//!
//! With the `parallel-layout` feature the layout is timed on thread pools of various sizes, and the
//! resulting layouts are checked to be identical. A fingerprint of the layout is printed so that
//! runs with and without the feature can be compared.
//!
//! Usage: `cargo run --release --example layout_bench [--features parallel-layout] -- <url> [width] [iterations]`

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_traits::shell::{ColorScheme, Viewport};
use reqwest::Url;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

/// Hash the final layout of every node in the layout tree. Also returns the number of nodes.
fn layout_fingerprint(doc: &BaseDocument) -> (u64, usize) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut count = 0;
    let mut stack = vec![doc.root_element().id];
    while let Some(node_id) = stack.pop() {
        let node = doc.get_node(node_id).unwrap();
        count += 1;
        if node.is_element() || node.is_anonymous() {
            let layout = node.final_layout();
            for value in [
                layout.location.x,
                layout.location.y,
                layout.size.width,
                layout.size.height,
                layout.scrollable_overflow_rect.right,
                layout.scrollable_overflow_rect.bottom,
            ] {
                value.to_bits().hash(&mut hasher);
            }
        }
        if let Some(children) = node.layout_children.borrow().as_ref() {
            stack.extend(children.iter().copied());
        }
    }
    (hasher.finish(), count)
}

/// Lay the document out from scratch `iterations` times. Returns the times and the layout's fingerprint.
fn time_layout(doc: &mut BaseDocument, iterations: usize) -> (Vec<Duration>, u64) {
    let mut times = Vec::new();
    for _ in 0..iterations {
        doc.invalidate_all_layout_caches();
        let start = Instant::now();
        doc.resolve_layout();
        times.push(start.elapsed());
    }
    times.sort();
    (times, layout_fingerprint(doc).0)
}

fn report(label: &str, times: &[Duration], fingerprint: u64, expected_fingerprint: u64) {
    let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
    let status = if fingerprint == expected_fingerprint {
        "same layout"
    } else {
        "LAYOUT DIFFERS"
    };
    println!(
        "{label:<28} min {:>8.3}ms  median {:>8.3}ms  ({status})",
        ms(times[0]),
        ms(times[times.len() / 2]),
    );
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let url_string = args
        .next()
        .unwrap_or_else(|| "https://www.google.com".into());
    let width: u32 = args.next().and_then(|arg| arg.parse().ok()).unwrap_or(1200);
    let iterations: usize = args.next().and_then(|arg| arg.parse().ok()).unwrap_or(20);

    let url = Url::parse(&url_string)
        .or_else(|_| Url::from_file_path(std::fs::canonicalize(&url_string).unwrap_or_default()))
        .or_else(|_| Url::parse(&format!("https://{url_string}")))
        .expect("Invalid url");
    let url_string = url.to_string();

    let html = match url.scheme() {
        "file" => String::from_utf8(std::fs::read(url.path()).unwrap()).unwrap(),
        _ => {
            let client = reqwest::Client::new();
            let request = client.get(url).header("User-Agent", USER_AGENT);
            request.send().await.unwrap().text().await.unwrap()
        }
    };

    let scale = 1.0;
    let net = Arc::new(Provider::new(None));
    let mut document = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some(url_string.clone()),
            net_provider: Some(Arc::clone(&net) as _),
            viewport: Some(Viewport::new(width, 800, scale, ColorScheme::Light)),
            ..Default::default()
        },
    );

    // Fetch assets, then resolve styles and layout
    let fetch_start = Instant::now();
    loop {
        document.resolve(0.0);
        if net.is_empty() || fetch_start.elapsed() > Duration::from_secs(30) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    document.resolve(0.0);
    let doc: &mut BaseDocument = &mut document;

    let (fingerprint, node_count) = layout_fingerprint(doc);
    let height = doc.root_element().final_layout().size.height;
    println!("{url_string}");
    println!("{node_count} layout nodes, {width}x{height}, layout fingerprint {fingerprint:016x}");

    #[cfg(not(feature = "parallel-layout"))]
    {
        let (times, new_fingerprint) = time_layout(doc, iterations);
        report("sequential", &times, new_fingerprint, fingerprint);
    }

    #[cfg(feature = "parallel-layout")]
    {
        doc.set_parallel_layout_threshold(None);
        let (times, new_fingerprint) = time_layout(doc, iterations);
        report(
            "batched, not parallel",
            &times,
            new_fingerprint,
            fingerprint,
        );

        // Sweep: `WEIGHTS=leaf,container,inline_root,text_bytes_per_unit` and `THRESHOLDS=a,b,c`
        let weights = std::env::var("WEIGHTS").ok().map(|weights| {
            let w: Vec<u32> = weights.split(',').map(|w| w.parse().unwrap()).collect();
            blitz_dom::ParallelLayoutWeights {
                leaf: w[0],
                container: w[1],
                inline_root: w[2],
                text_bytes_per_unit: w[3],
            }
        });
        if let Some(weights) = weights {
            println!("{weights:?}");
            doc.set_parallel_layout_weights(weights);
        }
        let thresholds: Vec<u32> = match std::env::var("THRESHOLDS") {
            Ok(thresholds) => thresholds.split(',').map(|t| t.parse().unwrap()).collect(),
            Err(_) => vec![256],
        };
        let thread_counts: Vec<usize> = match std::env::var("THREADS") {
            Ok(threads) => threads.split(',').map(|t| t.parse().unwrap()).collect(),
            Err(_) => vec![1, 2, 4, 8],
        };
        for thread_count in thread_counts {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(thread_count)
                .build()
                .unwrap();
            for &threshold in &thresholds {
                doc.set_parallel_layout_threshold(Some(threshold));
                // The document is not `Send`, but it is only used by one thread
                // at a time (other than by the parallel layout code itself)
                struct AssertSend<T>(T);
                // SAFETY: see above
                unsafe impl<T> Send for AssertSend<T> {}
                let doc_ref = AssertSend(&mut *doc);
                let (times, new_fingerprint) = pool.install(move || {
                    let doc_ref = doc_ref;
                    time_layout(doc_ref.0, iterations)
                });
                let label = format!("{thread_count} threads, threshold {threshold}");
                report(&label, &times, new_fingerprint, fingerprint);
            }
        }
    }
}
