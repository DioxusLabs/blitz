#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use blitz_dom::net::Resource;
use blitz_traits::navigation::{DummyNavigationProvider, NavigationProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use panic_backtrace::StashedPanicInfo;
use parley::FontContext;
use report::{generate_expectations, generate_report};
use supports_hyperlinks::Stream as HyperlinkStream;
use terminal_link::Link;
use test_runners::{SubtestResult, process_test_file};
use thread_local::ThreadLocal;
use url::Url;

use rayon::prelude::*;
use regex::Regex;

use bitflags::bitflags;
use log::{error, info};
use owo_colors::OwoColorize;
use std::cell::RefCell;
use std::fmt::Display;
use std::fs::File;
use std::io::{BufWriter, IsTerminal, Write, stdout};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{self, Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::Ordering;
use std::sync::atomic::{AtomicU32, AtomicUsize};
use std::time::{Duration, Instant, SystemTime};
use std::{env, fs};

mod test_runners;

mod manifest;
mod net_provider;
mod panic_backtrace;
mod renderer;
mod report;

#[cfg(test)]
use manifest::TestType;
use manifest::{Test, split_test_url};
use net_provider::WptNetProvider;
use renderer::{Renderer, RendererKind};

/// Create a unix timestamp of the current time using the standard library
fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Whether to wrap test names in OSC 8 hyperlink escape sequences.
///
/// `supports_hyperlinks::supports_hyperlinks()` only sniffs environment variables, so it returns
/// `true` even when stdout is redirected to a file or a pipe. That leaks half-written hyperlink
/// sequences into captured output: if the consumer of that output truncates it (or dies mid-line,
/// e.g. `| head`), the terminal never sees the closing `OSC 8 ; ; ST` and styles all subsequent
/// output as a link. `supports_hyperlinks::on` additionally requires stdout to be a terminal
/// (while still honouring the `FORCE_HYPERLINK` override).
static USE_HYPERLINKS: LazyLock<bool> =
    LazyLock::new(|| supports_hyperlinks::on(HyperlinkStream::Stdout));

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const SCALE: f64 = 1.0;

bitflags! {
    #[derive(Copy, Clone)]
    pub struct TestFlags : u32 {
        const USES_FLOAT = 0b00000001;
        const USES_INTRINSIC_SIZE = 0b00000010;
        const USES_CALC = 0b00000100;
        const USES_DIRECTION = 0b00001000;
        const USES_WRITING_MODE = 0b00010000;
        const USES_SUBGRID = 0b00100000;
        const USES_GRID_LANES = 0b01000000;
        const USES_SCRIPT = 0b10000000;
    }
}

#[derive(Copy, Clone, PartialEq)]
enum TestKind {
    Ref,
    Attr,
    Crash,
    TestHarness,
    Unknown,
}

impl Display for TestKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestKind::Ref => f.write_str("REF"),
            TestKind::Attr => f.write_str("ATT"),
            TestKind::Crash => f.write_str("CRA"),
            TestKind::TestHarness => f.write_str("HAR"),
            TestKind::Unknown => f.write_str("UNK"),
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
enum TestStatus {
    Pass,
    Fail,
    Timeout,
    Skip,
    Crash,
}

impl TestStatus {
    fn as_str(&self) -> &'static str {
        match self {
            TestStatus::Pass => "PASS",
            TestStatus::Fail => "FAIL",
            TestStatus::Timeout => "TIMEOUT",
            TestStatus::Skip => "SKIP",
            TestStatus::Crash => "CRASH",
        }
    }
}

#[derive(Copy, Clone)]
struct SubtestCounts {
    pass: u32,
    total: u32,
}

impl SubtestCounts {
    /// 1 of 1 subtests pass. Indicates PASS for a test with no subtests
    const ONE_OF_ONE: Self = Self { pass: 1, total: 1 };
    /// 0 of 1 subtests pass. Indicates FAIL for a test with no subtests
    const ZERO_OF_ONE: Self = Self { pass: 0, total: 1 };
    /// 0 of 0 subtests pass. Indicates the test was SKIPed
    const ZERO_OF_ZERO: Self = Self { pass: 0, total: 0 };

    fn pass_fraction(self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.pass as f64) / (self.total as f64)
        }
    }

    fn as_status(self) -> TestStatus {
        if self.total == 0 {
            TestStatus::Skip
        } else if self.total == self.pass {
            TestStatus::Pass
        } else {
            TestStatus::Fail
        }
    }
}

const BLOCKED_TESTS: &[&str] = &[
    // This test currently causes a wgpu validation error:
    // "Buffer size 17179869184 is greater than the maximum buffer size"
    "css/css-flexbox/flexbox-paint-ordering-002.xhtml",
    // Panics with: "Buffer length in `ImageBuffer::new` overflows usize"
    "css/css-sizing/aspect-ratio/zero-or-infinity-006.html",
    "css/css-sizing/aspect-ratio/zero-or-infinity-009.html",
    "css/css-sizing/aspect-ratio/zero-or-infinity-010.html",
    // Stack overflow: usvg's clipPath conversion recurses forever on
    // clip-paths that reference each other in a cycle
    "css/css-masking/clip-path-svg-content/clip-path-recursion-001.svg",
];

fn collect_tests(wpt_dir: &Path, mut suites: Vec<String>) -> Vec<Test> {
    if suites.is_empty() {
        suites.push("css/css-flexbox".to_string());
        suites.push("css/css-grid".to_string());
    }

    let all_tests = manifest::load_tests(wpt_dir).into_iter().filter(|test| {
        !BLOCKED_TESTS
            .iter()
            .any(|blocked| split_test_url(&test.url).0 == *blocked)
    });

    // "full" runs every test except the `encoding` suite
    if suites.iter().any(|suite| suite == "full") {
        return all_tests
            .filter(|test| !test.url.starts_with("encoding/"))
            .collect();
    }

    manifest::select_tests(wpt_dir, all_tests.collect(), &suites)
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
    worker_index: usize,
    run_quarantined: bool,
    viewport: Viewport,
    net_provider: Arc<WptNetProvider<Resource>>,
    navigation_provider: Arc<dyn NavigationProvider>,
    renderers: Vec<Renderer>,
    /// Index into `renderers` of the one the current test renders with
    active_renderer: usize,
    font_ctx: FontContext,
    buffers: Buffers,

    // Things that aren't really thread-specifc, but are convenient to store here
    attrtest_re: Regex,
    float_re: Regex,
    intrinsic_re: Regex,
    calc_re: Regex,
    direction_re: Regex,
    writing_mode_re: Regex,
    subgrid_re: Regex,
    grid_lanes_re: Regex,
    script_re: Regex,
    out_dir: PathBuf,
    wpt_dir: PathBuf,
    dummy_base_url: Url,
}

struct TestResult {
    name: String,
    kind: TestKind,
    flags: TestFlags,
    status: TestStatus,
    subtest_counts: SubtestCounts,
    subtest_results: Vec<SubtestResult>,
    duration: Duration,
    panic_info: Option<StashedPanicInfo>,
}

impl TestResult {
    fn print_to(&self, mut out: impl Write) {
        let result_str = if *USE_HYPERLINKS {
            let url = format!("https://wpt.live/{}", self.name);
            let link = Link::new(&self.name, &url);
            format!(
                "{} ({}/{}) {} ({}ms) ",
                self.status.as_str(),
                self.subtest_counts.pass,
                self.subtest_counts.total,
                link,
                self.duration.as_millis(),
            )
        } else {
            format!(
                "{} ({}/{}) {} ({}ms) ",
                self.status.as_str(),
                self.subtest_counts.pass,
                self.subtest_counts.total,
                self.name,
                self.duration.as_millis(),
            )
        };

        match self.status {
            TestStatus::Pass => write!(out, "{}", result_str.green()).unwrap(),
            // TestStatus::Fail if !self.flags.is_empty() => {
            TestStatus::Fail if self.subtest_counts.pass > 0 => {
                write!(out, "{}", result_str.yellow()).unwrap()
            }
            TestStatus::Fail => write!(out, "{}", result_str.red()).unwrap(),
            TestStatus::Timeout => write!(out, "{}", result_str.bright_red()).unwrap(),
            TestStatus::Skip => write!(out, "{}", result_str.bright_black()).unwrap(),
            TestStatus::Crash => write!(out, "{}", result_str.bright_magenta()).unwrap(),
        };

        // Write test kind
        write!(out, "{}", format_args!("{}", self.kind).bright_black()).unwrap();

        // Write flag markers

        let mut flags = self.flags;
        if self.kind != TestKind::Ref {
            flags.remove(TestFlags::USES_SCRIPT);
        }

        if !flags.is_empty() {
            write!(out, " {}", "(".bright_black()).unwrap();

            if self.flags.contains(TestFlags::USES_FLOAT) {
                write!(out, "{}", "F".bright_black()).unwrap();
            }
            if self.flags.contains(TestFlags::USES_INTRINSIC_SIZE) {
                write!(out, "{}", "I".bright_black()).unwrap();
            }
            if self.flags.contains(TestFlags::USES_CALC) {
                write!(out, "{}", "C".bright_black()).unwrap();
            }
            if self.flags.contains(TestFlags::USES_DIRECTION) {
                write!(out, "{}", "D".bright_black()).unwrap();
            }
            if self.flags.contains(TestFlags::USES_WRITING_MODE) {
                write!(out, "{}", "W".bright_black()).unwrap();
            }
            if self.flags.contains(TestFlags::USES_SUBGRID) {
                write!(out, "{}", "S".bright_black()).unwrap();
            }
            if self.flags.contains(TestFlags::USES_GRID_LANES) {
                write!(out, "{}", "M".bright_black()).unwrap();
            }
            if self.kind == TestKind::Ref && self.flags.contains(TestFlags::USES_SCRIPT) {
                write!(out, "{}", "X".bright_black()).unwrap();
            }

            write!(out, "{}", ")".bright_black()).unwrap();
        }

        // Newline
        writeln!(out).unwrap();

        if let Some(panic_info) = &self.panic_info {
            if let Some(panic_msg) = &panic_info.message {
                writeln!(out, "{panic_msg}").unwrap();
            }
            writeln!(
                out,
                "Panicked at {}:{}:{}",
                panic_info.file, panic_info.line, panic_info.column
            )
            .unwrap();

            if let Some(trimmed_backtrace) = panic_backtrace::trim_backtrace(&panic_info.backtrace)
            {
                writeln!(out, "Backtrace:\n{trimmed_backtrace}").unwrap();
            }
        }
    }
}

fn main() {
    env_logger::init();
    std::panic::set_hook(Box::new(panic_backtrace::stash_panic_handler));

    let verbose = env::args().any(|arg| arg == "--verbose" || arg == "-v");
    let run_quarantined = env::args().any(|arg| arg == "--run-quarantined");
    let wpt_dir = path::absolute(env::var("WPT_DIR").expect("WPT_DIR is not set")).unwrap();
    info!("WPT_DIR: {}", wpt_dir.display());
    if !wpt_dir.exists() {
        error!(
            "WPT_DIR does not exist. This should be set to a local copy of https://github.com/web-platform-tests/wpt."
        );
    }
    let (renderer_kinds, suites) = renderer::parse_args(env::args().skip(1));
    let test_paths = collect_tests(&wpt_dir, suites);
    let count = test_paths.len();

    // `--list` prints the selected test URLs without running them
    if env::args().any(|arg| arg == "--list") {
        for test in &test_paths {
            println!("{}", test.url);
        }
        return;
    }

    let cargo_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = cargo_dir.parent().unwrap().join("output");
    if fs::exists(&out_dir).unwrap() {
        fs::remove_dir_all(&out_dir).unwrap();
    }
    fs::create_dir_all(&out_dir).unwrap();

    // With several renderers, each one's report and PNGs go in a subdirectory named after it.
    let renderer_dirs: Vec<PathBuf> = if renderer_kinds.len() > 1 {
        renderer_kinds
            .iter()
            .map(|kind| out_dir.join(kind.name()))
            .collect()
    } else {
        vec![out_dir.clone()]
    };

    let start = Instant::now();
    let start_timestamp = unix_timestamp();

    let num = AtomicU32::new(0);
    let completed_num = AtomicU32::new(0);

    let base_font_context = parley::FontContext::default();

    let thread_state: ThreadLocal<RefCell<ThreadCtx>> = ThreadLocal::new();
    let worker_counter = AtomicUsize::new(0);
    let stdout_is_terminal = stdout().is_terminal();

    if !verbose && stdout_is_terminal {
        let mut out = stdout().lock();
        for _ in 0..rayon::current_num_threads() {
            writeln!(out).unwrap();
        }
        out.flush().unwrap();
    }

    let results: Vec<Vec<TestResult>> = test_paths
        .into_par_iter()
        .map(|test| {
            let mut ctx = thread_state
                .get_or(|| {
                    let worker_index = worker_counter.fetch_add(1, Ordering::Relaxed);
                    let font_ctx = base_font_context.clone();
                    let test_buffer = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
                    let ref_buffer = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
                    let viewport = Viewport::new(
                        (WIDTH as f64 * SCALE).floor() as u32,
                        (HEIGHT as f64 * SCALE).floor() as u32,
                        SCALE as f32,
                        ColorScheme::Light,
                    );
                    let net_provider = Arc::new(WptNetProvider::new(&wpt_dir));

                    let float_re = Regex::new(r#"float:"#).unwrap();
                    let intrinsic_re =
                        Regex::new(r#"(width|height): ?(min|max|fit)-content"#).unwrap();
                    let calc_re = Regex::new(r#"calc\("#).unwrap();
                    let direction_re = Regex::new(r#"direction:|directionRTL"#).unwrap();
                    let writing_mode_re = Regex::new(r#"writing-mode:|vertical(RL|LR)"#).unwrap();
                    let subgrid_re = Regex::new(r#"subgrid"#).unwrap();
                    let grid_lanes_re = Regex::new(r#"grid-lanes"#).unwrap();
                    let script_re = Regex::new(r#"<script|onload="#).unwrap();

                    let attrtest_re =
                        Regex::new(r#"checkLayout\(\s*['"]([^'"]*)['"]\s*(,\s*(true|false))?\)"#)
                            .unwrap();

                    let dummy_base_url = Url::parse("http://dummy.local").unwrap();
                    let navigation_provider = Arc::new(DummyNavigationProvider);

                    RefCell::new(ThreadCtx {
                        worker_index,
                        run_quarantined,
                        viewport,
                        net_provider,
                        renderers: renderer_kinds
                            .iter()
                            .map(|&kind| Renderer::new(kind, WIDTH, HEIGHT))
                            .collect(),
                        active_renderer: 0,
                        font_ctx,
                        buffers: Buffers {
                            test_buffer,
                            ref_buffer,
                        },
                        attrtest_re,
                        float_re,
                        intrinsic_re,
                        calc_re,
                        direction_re,
                        writing_mode_re,
                        subgrid_re,
                        grid_lanes_re,
                        script_re,
                        out_dir: renderer_dirs[0].clone(),
                        wpt_dir: wpt_dir.clone(),
                        dummy_base_url,
                        navigation_provider,
                    })
                })
                .borrow_mut();

            let num = num.fetch_add(1, Ordering::Relaxed) + 1;

            let relative_path = test.path
                .strip_prefix(&ctx.wpt_dir)
                .unwrap()
                .to_string_lossy()
                .replace("\\", "/");

            let mut test_results = Vec::with_capacity(renderer_kinds.len());
            for (index, out_dir) in renderer_dirs.iter().enumerate() {
                ctx.active_renderer = index;
                ctx.out_dir.clone_from(out_dir);

                // Clear any pending requests to avoid failed requests from a previous test interfering with subsequent tests
                ctx.net_provider.reset();

                let start = Instant::now();

                let result = catch_unwind(AssertUnwindSafe(|| {
                    panic_backtrace::backtrace_cutoff(|| process_test_file(&mut ctx, &relative_path, &test.url, &test.test_type))
                }));
                let (kind, flags, status, subtest_counts, panic_info, subtest_results) = match result {
                    Ok((kind, flags, status, subtest_counts, subtest_results)) => {
                        (kind, flags, status, subtest_counts, None, subtest_results)
                    }
                    Err(_) => {
                        let panic_info = panic_backtrace::take_stashed_panic_info();
                        (
                            TestKind::Unknown,
                            TestFlags::empty(),
                            TestStatus::Crash,
                            SubtestCounts::ZERO_OF_ZERO,
                            panic_info,
                            Vec::new(),
                        )
                    }
                };

                test_results.push(TestResult {
                    name: test.url.clone(),
                    kind,
                    flags,
                    status,
                    subtest_counts,
                    subtest_results,
                    duration: start.elapsed(),
                    panic_info,
                });
            }

            if verbose {
                // Print status line
                let mut out = stdout().lock();
                for (kind, result) in renderer_kinds.iter().zip(&test_results) {
                    write!(out, "[{num}/{count}] ").unwrap();
                    if renderer_kinds.len() > 1 {
                        write!(out, "{} ", kind.name()).unwrap();
                    }
                    result.print_to(&mut out);
                }
            } else {
                let completed_num = completed_num.fetch_add(1, Ordering::Relaxed) + 1;
                if stdout_is_terminal {
                    let worker_index = ctx.worker_index;
                    let worker_count = rayon::current_num_threads();
                    let up = worker_count - worker_index;
                    let statuses: Vec<_> = test_results.iter().map(|result| result.status.as_str()).collect();
                    let mut out = stdout().lock();
                    write!(
                        out,
                        "\x1b[?7l\x1b[{up}A\x1b[2K\r[{completed_num}/{count}] thread {worker_index:>2}: {} {}\x1b[{up}B\r\x1b[?7h",
                        statuses.join("/"),
                        test.url
                    )
                    .unwrap();
                    out.flush().unwrap();
                } else if completed_num.is_multiple_of(1000) || completed_num == count as u32 {
                    println!("[{completed_num}/{count}] ...");
                }
            }

            test_results
        })
        .collect();

    let end_timestamp = unix_timestamp();
    let duration = start.elapsed();

    let mut results_by_renderer: Vec<Vec<TestResult>> = renderer_kinds
        .iter()
        .map(|_| Vec::with_capacity(count))
        .collect();
    for test_results in results {
        for (results, result) in results_by_renderer.iter_mut().zip(test_results) {
            results.push(result);
        }
    }

    let mut statuses = Vec::new();
    for ((kind, mut results), dir) in renderer_kinds
        .iter()
        .zip(results_by_renderer)
        .zip(&renderer_dirs)
    {
        // Sort results alphabetically
        results.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        if renderer_kinds.len() > 1 {
            println!("\n\n{}\n{}", kind.name(), "=".repeat(kind.name().len()));
        }
        print_results(&results, duration);
        statuses.push(
            results
                .iter()
                .map(|result| (result.name.clone(), result.status))
                .collect::<Vec<_>>(),
        );
        write_outputs(dir, &wpt_dir, results, start_timestamp, end_timestamp);
    }

    if renderer_kinds.len() > 1 {
        print_renderer_differences(&renderer_kinds, &statuses, &out_dir);
    }
}

fn print_results(results: &[TestResult], duration: Duration) {
    let count = results.len() as u32;

    println!("\n\n\n\n\nOrdered Results\n===============\n");
    let mut out = stdout().lock();
    for (num, test) in results.iter().enumerate() {
        write!(out, "[{num:0>4}/{count}] ").unwrap();
        test.print_to(&mut out);
    }
    drop(out);

    let count_status = |status| results.iter().filter(|r| r.status == status).count() as u32;
    let pass_count = count_status(TestStatus::Pass);
    let fail_count = count_status(TestStatus::Fail);
    let timeout_count = count_status(TestStatus::Timeout);
    let crash_count = count_status(TestStatus::Crash);
    let skip_count = count_status(TestStatus::Skip);
    let run_count = pass_count + fail_count + timeout_count + crash_count;

    let fractional_pass_count: f64 = results
        .iter()
        .map(|r| r.subtest_counts.pass_fraction())
        .sum();
    let subtest_count: u32 = results.iter().map(|r| r.subtest_counts.total).sum();
    let subtest_pass_count: u32 = results.iter().map(|r| r.subtest_counts.pass).sum();

    let mut grid_lanes_fail_count = 0;
    let mut subgrid_fail_count = 0;
    let mut writing_mode_fail_count = 0;
    let mut direction_fail_count = 0;
    let mut float_fail_count = 0;
    let mut calc_fail_count = 0;
    let mut intrinsic_size_fail_count = 0;
    let mut script_fail_count = 0;
    let mut other_fail_count = 0;
    for result in results.iter().filter(|r| r.status == TestStatus::Fail) {
        let flags = result.flags;
        if flags.contains(TestFlags::USES_GRID_LANES) {
            grid_lanes_fail_count += 1;
        } else if flags.contains(TestFlags::USES_SUBGRID) {
            subgrid_fail_count += 1;
        } else if flags.contains(TestFlags::USES_WRITING_MODE) {
            writing_mode_fail_count += 1;
        } else if flags.contains(TestFlags::USES_DIRECTION) {
            direction_fail_count += 1;
        } else if flags.contains(TestFlags::USES_INTRINSIC_SIZE) {
            intrinsic_size_fail_count += 1;
        } else if flags.contains(TestFlags::USES_CALC) {
            calc_fail_count += 1;
        } else if flags.contains(TestFlags::USES_FLOAT) {
            float_fail_count += 1;
        } else if result.kind == TestKind::Ref && flags.contains(TestFlags::USES_SCRIPT) {
            script_fail_count += 1;
        } else {
            other_fail_count += 1;
        }
    }

    fn as_percent(amount: u32, out_of: u32) -> f32 {
        (amount as f32 / out_of as f32) * 100.0
    }

    let run_percent = as_percent(run_count, count);
    let skip_percent = as_percent(skip_count, count);
    let pass_percent_run = as_percent(pass_count, run_count);
    let pass_percent_total = as_percent(pass_count, count);
    let fractional_pass_percent_run = as_percent(fractional_pass_count as u32, run_count);
    let fractional_pass_percent_total = as_percent(fractional_pass_count as u32, count);
    let fail_percent_run = as_percent(fail_count, run_count);
    let fail_percent_total = as_percent(fail_count, count);
    let timeout_percent_run = as_percent(timeout_count, run_count);
    let timeout_percent_total = as_percent(timeout_count, count);
    let crash_percent_run = as_percent(crash_count, run_count);
    let crash_percent_total = as_percent(crash_count, count);

    let subtest_pass_percent = as_percent(subtest_pass_count, subtest_count);

    println!("Done in {:.2}s", duration.as_millis() as f64 / 1000.0);
    println!("---\n");

    println!("{count:>4} tests FOUND");
    println!("{skip_count:>4} tests SKIPPED ({skip_percent:.2}%)");
    println!("{run_count:>4} tests RUN ({run_percent:.2}%)");

    println!("{}", "\nOf tests run:".bright_black());
    println!("{subtest_count:>4} subtests RUN");
    println!("{subtest_pass_count:>4} subtests PASSED ({subtest_pass_percent:.2}%)");

    println!("{}", "\nOf tests run:".bright_black());
    println!(
        "{crash_count:>4} tests CRASHED ({crash_percent_run:.2}% of run; {crash_percent_total:.2}% of found)"
    );
    println!(
        "{pass_count:>4} tests PASSED ({pass_percent_run:.2}% of run; {pass_percent_total:.2}% of found)"
    );
    println!(
        "{fail_count:>4} tests FAILED ({fail_percent_run:.2}% of run; {fail_percent_total:.2}% of found)"
    );
    println!(
        "{timeout_count:>4} tests TIMED OUT ({timeout_percent_run:.2}% of run; {timeout_percent_total:.2}% of found)"
    );

    println!("{}", "\nCounting partial tests:".bright_black());
    println!(
        "{fractional_pass_count:>4.2} tests PASSED ({fractional_pass_percent_run:.2}% of run; {fractional_pass_percent_total:.2}% of found)"
    );

    println!("{}", "\nOf those tests which failed:".bright_black());
    println!("{other_fail_count:>4} do not use unsupported features");
    println!("{writing_mode_fail_count:>4} use writing-mode (W)");
    println!("{direction_fail_count:>4} use direction (D)");
    println!("{float_fail_count:>4} use floats (F)");
    println!("{intrinsic_size_fail_count:>4} use intrinsic size keywords (I)");
    println!("{script_fail_count:>4} use script (X)");
    println!("{calc_fail_count:>4} use calc (C)");
    if subgrid_fail_count > 0 {
        println!("{subgrid_fail_count:>4} use subgrid (S)");
    }
    if grid_lanes_fail_count > 0 {
        println!("{grid_lanes_fail_count:>4} use grid-lanes (M)");
    }
}

fn write_outputs(
    out_dir: &Path,
    wpt_dir: &Path,
    results: Vec<TestResult>,
    start_timestamp: u64,
    end_timestamp: u64,
) {
    fs::create_dir_all(out_dir).unwrap();

    // Generate wpt_expectations.txt
    let expectations = generate_expectations(&results);
    let expectations_path = out_dir.join("wpt_expectations.txt");
    fs::write(&expectations_path, expectations).unwrap();

    // Generate wptreport.json
    let report_start = Instant::now();
    let report = generate_report(wpt_dir, results, start_timestamp, end_timestamp);
    println!(
        "\nReport generated in {}ms",
        report_start.elapsed().as_millis()
    );
    let write_report_start = Instant::now();
    let report_path = out_dir.join("wptreport.json");
    let mut report_file_writer = BufWriter::new(File::create(&report_path).unwrap());
    serde_json::to_writer(&mut report_file_writer, &report).unwrap();
    println!(
        "Report written to {:?} in {}ms",
        report_path,
        write_report_start.elapsed().as_millis()
    );
}

/// Prints each renderer's pass count and the tests whose status differs between renderers, and
/// writes the latter to `renderer_differences.txt`.
fn print_renderer_differences(
    kinds: &[RendererKind],
    statuses: &[Vec<(String, TestStatus)>],
    out_dir: &Path,
) {
    println!("\n\nRenderers\n=========\n");
    for (kind, statuses) in kinds.iter().zip(statuses) {
        let passed = statuses
            .iter()
            .filter(|(_, status)| *status == TestStatus::Pass)
            .count();
        println!("{passed:>6} / {} PASS with {}", statuses.len(), kind.name());
    }

    let mut differences = String::new();
    for (index, (name, status)) in statuses[0].iter().enumerate() {
        if statuses[1..].iter().any(|other| other[index].1 != *status) {
            differences.push_str(name);
            for (kind, statuses) in kinds.iter().zip(statuses) {
                differences.push_str(&format!(" {}:{}", kind.name(), statuses[index].1.as_str()));
            }
            differences.push('\n');
        }
    }
    let path = out_dir.join("renderer_differences.txt");
    fs::write(&path, &differences).unwrap();
    println!(
        "\n{} tests differ between renderers, listed in {}",
        differences.lines().count(),
        path.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const HARNESS: &str = "<script src=/resources/testharness.js></script>";

    fn write_suite() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let suite = dir.path().join("suite");
        fs::create_dir(&suite).unwrap();
        let files = [
            (
                "test.html",
                format!("<meta name=variant content='?one'><meta name=variant content='?two#part'>{HARNESS}"),
            ),
            ("test.any.js", "// META: variant=?js\ntest(() => {});".into()),
            ("test.window.js", "// META: variant=?window\ntest(() => {});".into()),
            ("plain.html", "<p>No test</p>".into()),
            (
                "test.xhtml",
                "<html xmlns='http://www.w3.org/1999/xhtml'><meta name='variant' content='?xml'/><script src='/resources/testharness.js'/></html>".into(),
            ),
            ("ref.html", "<link rel=match href=ref-ref.html><meta name=fuzzy content='0-2;0-10'>".into()),
            ("ref-ref.html", "<p>Reference</p>".into()),
            ("foo-crash.html", "<p>Crash</p>".into()),
        ];
        for (name, contents) in files {
            fs::write(suite.join(name), contents).unwrap();
        }
        dir
    }

    fn urls(tests: &[Test]) -> Vec<&str> {
        tests.iter().map(|test| test.url.as_str()).collect()
    }

    #[test]
    fn discovers_tests_from_manifest() {
        let dir = write_suite();
        let tests = collect_tests(dir.path(), vec!["suite".into()]);
        assert_eq!(
            urls(&tests),
            [
                "suite/foo-crash.html",
                "suite/ref.html",
                "suite/test.any.html?js",
                "suite/test.html?one",
                "suite/test.html?two#part",
                "suite/test.window.html?window",
                "suite/test.xhtml?xml"
            ]
        );
        assert!(matches!(tests[0].test_type, TestType::Crash));
        let TestType::Ref {
            match_references,
            mismatch_references,
            fuzzy,
        } = &tests[1].test_type
        else {
            panic!("ref.html is not a reftest");
        };
        assert_eq!(match_references, &["/suite/ref-ref.html"]);
        assert!(mismatch_references.is_empty());
        assert_eq!(fuzzy.len(), 1);
        assert_eq!(fuzzy[0].reference, None);
        assert_eq!(fuzzy[0].tolerance.total_pixels.max, 10);
        assert!(matches!(tests[2].test_type, TestType::TestHarness));
        assert_eq!(tests[2].path, dir.path().join("suite/test.any.js"));
    }

    #[test]
    fn selects_files_globs_and_explicit_urls() {
        let dir = write_suite();
        let suite = dir.path().join("suite");

        let tests = collect_tests(
            dir.path(),
            vec!["suite/test.html".into(), "suite/*.xhtml".into()],
        );
        assert_eq!(
            urls(&tests),
            [
                "suite/test.html?one",
                "suite/test.html?two#part",
                "suite/test.xhtml?xml"
            ]
        );
        assert!(collect_tests(dir.path(), vec!["sui*".into()]).len() == 7);
        assert!(collect_tests(dir.path(), vec!["*.html".into()]).is_empty());

        let tests = collect_tests(
            dir.path(),
            vec![
                "suite/test.html?override#hash".into(),
                "suite/test.any.html?override".into(),
                "suite/test.window.html?override".into(),
                "suite/ref.html?override".into(),
            ],
        );
        assert_eq!(
            urls(&tests),
            [
                "suite/ref.html?override",
                "suite/test.any.html?override",
                "suite/test.html?override#hash",
                "suite/test.window.html?override"
            ]
        );
        let TestType::Ref {
            match_references, ..
        } = &tests[0].test_type
        else {
            panic!("ref.html is not a reftest");
        };
        assert_eq!(match_references, &["/suite/ref-ref.html?override"]);
        let tests = &tests[1..];
        assert_eq!(tests[0].path, suite.join("test.any.js"));
        assert_eq!(tests[1].path, suite.join("test.html"));
        assert_eq!(tests[2].path, suite.join("test.window.js"));
    }
}
