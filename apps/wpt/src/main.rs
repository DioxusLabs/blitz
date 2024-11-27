use blitz_dom::net::Resource;
use blitz_dom::Viewport;
use blitz_renderer_vello::VelloImageRenderer;
use parley::FontContext;
use thread_local::ThreadLocal;
use url::Url;

use rayon::prelude::*;
use regex::Regex;

use log::{error, info};
use owo_colors::OwoColorize;
use std::cell::RefCell;
use std::fmt::Display;
use std::io::{stdout, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{self, Path, PathBuf};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{env, fs};

mod attr_test;
mod net_provider;
mod ref_test;

use attr_test::process_attr_test;
use net_provider::WptNetProvider;
use ref_test::process_ref_test;

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const SCALE: f64 = 1.0;

#[derive(Copy, Clone)]
enum TestKind {
    Ref,
    Attr,
    Unknown,
}

impl Display for TestKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestKind::Ref => f.write_str("ref"),
            TestKind::Attr => f.write_str("attr"),
            TestKind::Unknown => f.write_str("unknown"),
        }
    }
}

#[derive(Copy, Clone)]
enum TestStatus {
    Pass,
    Fail,
    Skip,
    Crash,
}

impl TestStatus {
    fn as_str(&self) -> &'static str {
        match self {
            TestStatus::Pass => "PASS ",
            TestStatus::Fail => "FAIL ",
            TestStatus::Skip => "SKIP ",
            TestStatus::Crash => "CRASH",
        }
    }
}

const BLOCKED_TESTS: &[&str] = &[
    // This test currently causes a wgpu validation error:
    // "Buffer size 17179869184 is greater than the maximum buffer size"
    "css/css-flexbox/flexbox-paint-ordering-002.xhtml",
    // Panics with: "Buffer length in `ImageBuffer::new` overflows usize"
    "css/css-sizing/aspect-ratio/zero-or-infinity-006.html",
    "css/css-sizing/aspect-ratio/zero-or-infinity-010.html",
];

fn path_contains_directory(path: &Path, dir_name: &str) -> bool {
    path.components()
        .any(|component| component.as_os_str() == dir_name)
}

fn collect_tests(wpt_dir: &Path) -> Vec<PathBuf> {
    let mut test_paths = Vec::new();

    let mut suites: Vec<_> = std::env::args().skip(1).collect();
    if suites.is_empty() {
        suites.push("css/css-flexbox".to_string());
        suites.push("css/css-grid".to_string());
    }

    for suite in suites {
        for ext in ["htm", "html", "xht", "xhtml"] {
            let pattern = format!("{}/{}/**/*.{}", wpt_dir.display(), suite, ext);

            let glob_results = glob::glob(&pattern).expect("Invalid glob pattern.");

            test_paths.extend(glob_results.filter_map(|glob_result| {
                if let Ok(path_buf) = glob_result {
                    // let is_tentative = path_buf.ends_with("tentative.html");
                    let path_str = path_buf.to_string_lossy();
                    let is_ref = path_str.ends_with("-ref.html")
                        || path_str.ends_with("-ref.htm")
                        || path_str.ends_with("-ref.xhtml")
                        || path_str.ends_with("-ref.xht")
                        || path_contains_directory(&path_buf, "reference");
                    let is_support_file = path_contains_directory(&path_buf, "support");

                    let is_blocked = BLOCKED_TESTS
                        .iter()
                        .any(|suffix| path_str.ends_with(suffix));

                    if is_ref | is_support_file | is_blocked {
                        None
                    } else {
                        Some(path_buf)
                    }
                } else {
                    error!("Failure during glob.");
                    panic!("Failure during glob");
                }
            }));
        }
    }

    test_paths
}

fn clone_font_ctx(ctx: &FontContext) -> FontContext {
    FontContext {
        collection: ctx.collection.clone(),
        source_cache: ctx.source_cache.clone(),
    }
}

enum BufferKind {
    Test,
    Ref,
}
struct Buffers {
    pub test_buffer: Vec<u8>,
    pub ref_buffer: Vec<u8>,
}
impl Buffers {
    fn get_mut(&mut self, kind: BufferKind) -> &mut Vec<u8> {
        match kind {
            BufferKind::Test => &mut self.test_buffer,
            BufferKind::Ref => &mut self.ref_buffer,
        }
    }
}
struct ThreadCtx {
    viewport: Viewport,
    net_provider: Arc<WptNetProvider<Resource>>,
    renderer: VelloImageRenderer,
    font_ctx: FontContext,
    buffers: Buffers,

    // Things that aren't really thread-specifc, but are convenient to store here
    reftest_re: Regex,
    attrtest_re: Regex,
    out_dir: PathBuf,
    wpt_dir: PathBuf,
    dummy_base_url: Url,
}

struct TestResult {
    name: String,
    kind: TestKind,
    status: TestStatus,
    duration: Duration,
    panic_msg: Option<String>,
}

impl TestResult {
    fn print_to(&self, mut out: impl Write) {
        let result_str = format!(
            "{} {} ({}) ({}ms)",
            self.status.as_str(),
            &self.name,
            self.kind,
            self.duration.as_millis()
        );
        match self.status {
            TestStatus::Pass => writeln!(out, "{}", result_str.green()).unwrap(),
            TestStatus::Fail => writeln!(out, "{}", result_str.red()).unwrap(),
            TestStatus::Skip => writeln!(out, "{}", result_str.bright_black()).unwrap(),
            TestStatus::Crash => writeln!(out, "{}", result_str.bright_magenta()).unwrap(),
        };
        if let Some(panic_msg) = &self.panic_msg {
            writeln!(out, "{}", panic_msg).unwrap();
        }
    }
}

fn main() {
    env_logger::init();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    let _guard = rt.enter();

    let wpt_dir = path::absolute(env::var("WPT_DIR").expect("WPT_DIR is not set")).unwrap();
    info!("WPT_DIR: {}", wpt_dir.display());
    if !wpt_dir.exists() {
        error!("WPT_DIR does not exist. This should be set to a local copy of https://github.com/web-platform-tests/wpt.");
    }
    let test_paths = collect_tests(&wpt_dir);
    let count = test_paths.len();

    let cargo_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = cargo_dir.join("output");
    if fs::exists(&out_dir).unwrap() {
        fs::remove_dir_all(&out_dir).unwrap();
    }
    fs::create_dir(&out_dir).unwrap();

    let pass_count = AtomicU32::new(0);
    let fail_count = AtomicU32::new(0);
    let skip_count = AtomicU32::new(0);
    let crash_count = AtomicU32::new(0);
    let start = Instant::now();

    let num = AtomicU32::new(0);

    let base_font_context = parley::FontContext::default();

    let thread_state: ThreadLocal<RefCell<ThreadCtx>> = ThreadLocal::new();

    let mut results: Vec<TestResult> = test_paths
        .into_par_iter()
        .map_init(
            || rt.enter(),
            |_guard, path| {
                let mut ctx = thread_state
                    .get_or(|| {
                        let renderer = rt.block_on(VelloImageRenderer::new(WIDTH, HEIGHT, SCALE));
                        let font_ctx = clone_font_ctx(&base_font_context);
                        let test_buffer = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
                        let ref_buffer = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
                        let viewport = Viewport::new(
                            (WIDTH as f64 * SCALE).floor() as u32,
                            (HEIGHT as f64 * SCALE).floor() as u32,
                            SCALE as f32,
                        );
                        let net_provider = Arc::new(WptNetProvider::new(&wpt_dir));
                        let reftest_re = Regex::new(r#"<link\s+rel="match"\s+href="([^"]+)""#)
                            .expect("Failed to compile reftest regex");

                        let attrtest_re = Regex::new(
                            r#"checkLayout\(\s*['"]([^'"]*)['"]\s*(,\s*(true|false))?\)"#,
                        )
                        .expect("Failed to compile attrtest regex");

                        let dummy_base_url = Url::parse("http://dummy.local").unwrap();

                        RefCell::new(ThreadCtx {
                            viewport,
                            net_provider,
                            renderer,
                            font_ctx,
                            buffers: Buffers {
                                test_buffer,
                                ref_buffer,
                            },
                            reftest_re,
                            attrtest_re,
                            out_dir: out_dir.clone(),
                            wpt_dir: wpt_dir.clone(),
                            dummy_base_url,
                        })
                    })
                    .borrow_mut();

                let num = num.fetch_add(1, Ordering::SeqCst) + 1;

                let relative_path = path
                    .strip_prefix(&ctx.wpt_dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace("\\", "/");

                let start = Instant::now();

                let result = catch_unwind(AssertUnwindSafe(|| {
                    rt.block_on(process_test_file(&mut ctx, &relative_path))
                }));
                let (kind, status, panic_msg) = match result {
                    Ok((kind, status)) => (kind, status, None),
                    Err(err) => {
                        let str_msg = err.downcast_ref::<&str>().map(|s| s.to_string());
                        let string_msg = err.downcast_ref::<String>().map(|s| s.to_string());
                        let panic_msg = str_msg.or(string_msg);

                        (TestKind::Unknown, TestStatus::Crash, panic_msg)
                    }
                };

                // Bump counts
                match status {
                    TestStatus::Pass => pass_count.fetch_add(1, Ordering::SeqCst),
                    TestStatus::Fail => fail_count.fetch_add(1, Ordering::SeqCst),
                    TestStatus::Skip => skip_count.fetch_add(1, Ordering::SeqCst),
                    TestStatus::Crash => crash_count.fetch_add(1, Ordering::SeqCst),
                };

                let result = TestResult {
                    name: relative_path,
                    kind,
                    status,
                    duration: start.elapsed(),
                    panic_msg,
                };

                // Print status line
                let mut out = stdout().lock();
                write!(out, "[{num}/{count}] ").unwrap();
                result.print_to(out);

                result
            },
        )
        .collect();

    // Sort results alphabetically
    results.sort_unstable_by(|a, b| a.name.cmp(&b.name));

    println!("\n\n\n\n\nOrdered Results\n===============\n");

    let mut out = stdout().lock();
    for (num, test) in results.iter().enumerate() {
        write!(out, "[{num:0>4}/{count}] ").unwrap();
        test.print_to(&mut out);
    }
    drop(out);

    let pass_count = pass_count.load(Ordering::SeqCst);
    let fail_count = fail_count.load(Ordering::SeqCst);
    let crash_count = crash_count.load(Ordering::SeqCst);
    let skip_count = skip_count.load(Ordering::SeqCst);

    let run_count = pass_count + fail_count + crash_count;
    let run_percent = (run_count as f32 / count as f32) * 100.0;

    println!("---");
    println!("Done in {}s", (Instant::now() - start).as_secs());
    println!("{pass_count} tests PASSED.");
    println!("{fail_count} tests FAILED.");
    println!("{crash_count} tests CRASHED.");
    println!("{skip_count} tests SKIPPED.");
    println!("{run_count} or {count} ({run_percent:.2}%) tests run.");
    println!("---");
    let pessimistic_percent = (pass_count as f32 / count as f32) * 100.0;
    let optimistic_percent = (pass_count as f32 / run_count as f32) * 100.0;
    println!("Percent of total: {pessimistic_percent:.2}%");
    println!("Percent of run: {optimistic_percent:.2}%");
}

#[allow(clippy::too_many_arguments)]
async fn process_test_file(ctx: &mut ThreadCtx, relative_path: &str) -> (TestKind, TestStatus) {
    info!("Processing test file: {}", relative_path);

    let file_contents = fs::read_to_string(ctx.wpt_dir.join(relative_path)).unwrap();

    // Ref Test
    let reference = ctx
        .reftest_re
        .captures(&file_contents)
        .and_then(|captures| captures.get(1).map(|href| href.as_str().to_string()));
    if let Some(reference) = reference {
        let result = process_ref_test(
            ctx,
            relative_path,
            file_contents.as_str(),
            reference.as_str(),
        )
        .await;

        return (TestKind::Ref, result);
    }

    // Attr Test
    let mut matches = ctx.attrtest_re.captures_iter(&file_contents);
    let first = matches.next();
    let second = matches.next();
    if first.is_some() && second.is_none() {
        // TODO: handle tests with multiple calls to checkLayout.
        let captures = first.unwrap();
        let selector = captures.get(1).unwrap().as_str().to_string();
        drop(matches);

        let result = process_attr_test(ctx, &selector, &file_contents, relative_path).await;

        return (TestKind::Attr, result);
    }

    // TODO: Handle other test formats.
    (TestKind::Unknown, TestStatus::Skip)
}
