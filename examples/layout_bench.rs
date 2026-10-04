//! Time the layout of a web page: load the first CLI argument as a URL (or a path to an HTML file),
//! then repeatedly lay the document out from scratch.
//!
//! With the `parallel-layout` feature the layout is timed on thread pools of various sizes, and the
//! resulting layouts are checked to be identical. A fingerprint of the layout is printed so that
//! runs with and without the feature can be compared.
//!
//! Usage: `cargo run --release --example layout_bench [--features parallel-layout] -- <url> [width] [iterations]`
//!
//! Environment variables (with the `parallel-layout` feature):
//! - `THRESHOLDS=256,1024`, `THREADS=1,2,4,8`, `WEIGHTS=leaf,container,inline_root,text_bytes_per_unit`
//! - `SPLIT=weight`: divide the jobs of a batch between tasks by weight rather than by count
//! - `PHASES=1`: also print the time taken by the subtree weight walk, the layout and the rounding
//! - `IDLE_MS=16`: sleep before each layout, so that the pool's threads are asleep when it starts
//! - `KEEP_AWAKE=1`: keep the pool's other threads spinning for the duration of each layout
//! - `PROFILE=1`: instead of timing parallel layout, run every batch that would be parallel in order
//!   on one thread, and report the time that layout would take with unlimited threads (`tinf`), the
//!   share of the time spent outside of all batches, and the share spent laying out inline boxes.
//!   Each line is printed twice: `inline_whatif true` treats the inline boxes of an inline root as
//!   if they were laid out in parallel.

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_traits::shell::{ColorScheme, Viewport};
use reqwest::Url;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Set the QoS class of the current thread, which decides which cores macOS runs it on. By default
/// it is the highest class, so that the fastest cores are preferred. `QOS=background` confines the
/// thread to the slowest cores instead (and lowers its priority).
#[cfg(target_os = "macos")]
fn set_thread_qos() {
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    const QOS_CLASS_BACKGROUND: u32 = 0x09;
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    let qos_class = match std::env::var("QOS").as_deref() {
        Ok("background") => QOS_CLASS_BACKGROUND,
        _ => QOS_CLASS_USER_INTERACTIVE,
    };
    // SAFETY: only changes the scheduling class of the current thread
    unsafe { pthread_set_qos_class_self_np(qos_class, 0) };
}
#[cfg(not(target_os = "macos"))]
fn set_thread_qos() {}

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
///
/// `IDLE_MS=n` sleeps for n milliseconds before each layout (so that the threads of the pool are
/// asleep when it starts, as they would be in an application). With `KEEP_AWAKE=1` the other threads
/// of the current rayon pool are kept spinning for the duration of each layout, so that they never
/// have to be woken up.
fn time_layout(doc: &mut BaseDocument, iterations: usize) -> (Vec<Duration>, u64) {
    let idle = std::env::var("IDLE_MS")
        .ok()
        .map(|ms| Duration::from_secs_f64(ms.parse::<f64>().unwrap() / 1000.0));
    let keep_awake = std::env::var_os("KEEP_AWAKE").is_some();
    let mut times = Vec::new();
    #[cfg(feature = "parallel-layout")]
    let mut phases = [u64::MAX; 3];
    for _ in 0..iterations {
        doc.invalidate_all_layout_caches();
        if let Some(idle) = idle {
            std::thread::sleep(idle);
        }

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let spinners = match rayon::current_thread_index() {
            Some(this_thread) if keep_awake => {
                let (stop, active) = (stop.clone(), active.clone());
                rayon::spawn_broadcast(move |ctx| {
                    if ctx.index() == this_thread {
                        return;
                    }
                    active.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        if rayon::yield_now() != Some(rayon::Yield::Executed) {
                            std::hint::spin_loop();
                        }
                    }
                    active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                });
                rayon::current_num_threads() - 1
            }
            _ => 0,
        };
        while active.load(std::sync::atomic::Ordering::SeqCst) < spinners {
            std::hint::spin_loop();
        }

        let start = Instant::now();
        doc.resolve_layout();
        times.push(start.elapsed());

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        while active.load(std::sync::atomic::Ordering::SeqCst) > 0 {
            std::hint::spin_loop();
        }
        #[cfg(feature = "parallel-layout")]
        for (phase, best) in blitz_dom::LAYOUT_PHASE_NS.iter().zip(&mut phases) {
            *best = (*best).min(phase.load(std::sync::atomic::Ordering::Relaxed));
        }
    }
    times.sort();
    #[cfg(feature = "parallel-layout")]
    if std::env::var_os("PHASES").is_some() {
        println!(
            "PHASES weights {:.1}us layout {:.1}us round {:.1}us",
            phases[0] as f64 / 1e3,
            phases[1] as f64 / 1e3,
            phases[2] as f64 / 1e3
        );
    }
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
    set_thread_qos();
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
    #[cfg(feature = "parallel-layout")]
    println!(
        "root weight {:?}",
        doc.parallel_layout_subtree_weight(doc.root_element().id)
    );

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

        if std::env::var_os("PROFILE").is_some() {
            let thresholds: Vec<u32> = match std::env::var("THRESHOLDS") {
                Ok(thresholds) => thresholds.split(',').map(|t| t.parse().unwrap()).collect(),
                Err(_) => vec![256],
            };
            for threshold in thresholds {
                doc.set_parallel_layout_threshold(Some(threshold));
                for inline_whatif in [false, true] {
                    blitz_dom::PROFILE_INLINE_WHATIF
                        .store(inline_whatif, std::sync::atomic::Ordering::Relaxed);
                    let mut best = (u64::MAX, 0, 0, 0);
                    for _ in 0..iterations {
                        doc.invalidate_all_layout_caches();
                        blitz_dom::profile_begin();
                        let start = Instant::now();
                        doc.resolve_layout();
                        let total = start.elapsed().as_nanos() as u64;
                        let (saved, in_batches, inline_boxes) = blitz_dom::profile_end();
                        if total < best.0 {
                            best = (total, saved, in_batches, inline_boxes);
                        }
                    }
                    let (total, saved, in_batches, inline_boxes) = best;
                    let tinf = total - saved;
                    println!(
                        "PROFILE threshold {threshold} inline_whatif {inline_whatif} t1 {:.3}ms tinf {:.3}ms parallelism {:.2} outside_batches {:.1}% inline_boxes {:.1}%",
                        total as f64 / 1e6,
                        tinf as f64 / 1e6,
                        total as f64 / tinf as f64,
                        100.0 * (total - in_batches) as f64 / total as f64,
                        100.0 * inline_boxes as f64 / total as f64,
                    );
                }
            }
            return;
        }

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
        // `SPLIT=weight` divides the jobs of a batch between tasks by weight rather than by count
        let split_by_weight = std::env::var("SPLIT").as_deref() == Ok("weight");
        blitz_dom::SPLIT_BY_WEIGHT.store(split_by_weight, std::sync::atomic::Ordering::Relaxed);
        let thread_counts: Vec<usize> = match std::env::var("THREADS") {
            Ok(threads) => threads.split(',').map(|t| t.parse().unwrap()).collect(),
            Err(_) => vec![1, 2, 4, 8],
        };
        for thread_count in thread_counts {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(thread_count)
                .start_handler(|_| set_thread_qos())
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
