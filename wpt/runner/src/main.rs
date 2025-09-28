use anyrender::ImageRenderer as _;
#[cfg(feature = "gpu")]
use anyrender_vello::VelloImageRenderer;
#[cfg(feature = "cpu")]
use anyrender_vello_cpu::VelloCpuImageRenderer as VelloImageRenderer;
use atomic_float::AtomicF64;
use blitz_dom::net::Resource;
use blitz_traits::navigation::{DummyNavigationProvider, NavigationProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use panic_backtrace::StashedPanicInfo;
use parley::FontContext;
use report::{generate_expectations, generate_report};
use supports_hyperlinks::supports_hyperlinks;
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
use std::io::{BufWriter, Write, stdout};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{self, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};
use std::{env, fs};

mod test_runners;

mod net_provider;
mod panic_backtrace;
mod report;

use net_provider::WptNetProvider;

/// Create a unix timestamp of the current time using the standard library
fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

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
        const USES_MASONRY = 0b01000000;
        const USES_SCRIPT = 0b10000000;
    }
}

#[derive(Copy, Clone, PartialEq)]
enum TestKind {
    Ref,
    Attr,
    Unknown,
}

impl Display for TestKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestKind::Ref => f.write_str("REF"),
            TestKind::Attr => f.write_str("ATT"),
            TestKind::Unknown => f.write_str("UNK"),
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
            TestStatus::Pass => "PASS",
            TestStatus::Fail => "FAIL",
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
];

fn path_contains_directory(path: &Path, dir_name: &str) -> bool {
    path.components()
        .any(|component| component.as_os_str() == dir_name)
}

fn filter_path(p: &Path) -> bool {
    // let is_tentative = path_buf.ends_with("tentative.html");
    let path_str = p.to_string_lossy();
    let is_ref = path_str.ends_with("-ref.html")
        || path_str.ends_with("-ref.htm")
        || path_str.ends_with("-ref.xhtml")
        || path_str.ends_with("-ref.xht")
        || path_contains_directory(p, "reference");
    let is_support_file = path_contains_directory(p, "support");

    let is_blocked = BLOCKED_TESTS
        .iter()
        .any(|suffix| path_str.ends_with(suffix));

    let is_dir = p.is_dir();

    !(is_ref | is_support_file | is_blocked | is_dir)
}

fn collect_tests(wpt_dir: &Path) -> Vec<PathBuf> {
    let mut test_paths = Vec::new();

    let mut suites: Vec<_> = std::env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .collect();
    if suites.is_empty() {
        suites.push("css/css-flexbox".to_string());
        suites.push("css/css-grid".to_string());
    }

    for suite in suites {
        for pat in ["", "/**/*.htm", "/**/*.html", "/**/*.xht", "/**/*.xhtml"] {
            let pattern = format!("{}/{}{}", wpt_dir.display(), suite, pat);

            let glob_results = glob::glob(&pattern).expect("Invalid glob pattern.");

            test_paths.extend(
                glob_results
                    .map(|glob_result| {
                        if let Ok(path_buf) = glob_result {
                            path_buf
                        } else {
                            error!("Failure during glob.");
                            panic!("Failure during glob");
                        }
                    })
                    .filter(|path_buf| filter_path(path_buf)),
            );
        }
    }

    test_paths
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
    navigation_provider: Arc<dyn NavigationProvider>,
    renderer: VelloImageRenderer,
    font_ctx: FontContext,
    buffers: Buffers,

    // Things that aren't really thread-specifc, but are convenient to store here
    reftest_re: Regex,
    attrtest_re: Regex,
    float_re: Regex,
    intrinsic_re: Regex,
    calc_re: Regex,
    direction_re: Regex,
    writing_mode_re: Regex,
    subgrid_re: Regex,
    masonry_re: Regex,
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
        let result_str = if supports_hyperlinks() {
            let url = format!("https://wpt.live/{}", &self.name);
            let link = Link::new(&self.name, &url);
            format!(
                "{} ({}/{}) {} ({}ms) ",
                self.status.as_str(),
                self.subtest_counts.pass,
                self.subtest_counts.total,
                &link,
                self.duration.as_millis(),
            )
        } else {
            format!(
                "{} ({}/{}) {} ({}ms) ",
                self.status.as_str(),
                self.subtest_counts.pass,
                self.subtest_counts.total,
                &self.name,
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
            if self.flags.contains(TestFlags::USES_MASONRY) {
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

    let wpt_dir = path::absolute(env::var("WPT_DIR").expect("WPT_DIR is not set")).unwrap();
    info!("WPT_DIR: {}", wpt_dir.display());
    if !wpt_dir.exists() {
        error!(
            "WPT_DIR does not exist. This should be set to a local copy of https://github.com/web-platform-tests/wpt."
        );
    }
    let test_paths = collect_tests(&wpt_dir);
    let count = test_paths.len();

    let cargo_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = cargo_dir.parent().unwrap().join("output");
    if fs::exists(&out_dir).unwrap() {
        fs::remove_dir_all(&out_dir).unwrap();
    }
    fs::create_dir(&out_dir).unwrap();

    let pass_count = AtomicU32::new(0);
    let fail_count = AtomicU32::new(0);
    let skip_count = AtomicU32::new(0);
    let crash_count = AtomicU32::new(0);

    let subtest_count = AtomicU32::new(0);
    let subtest_pass_count = AtomicU32::new(0);
    let subtest_fail_count = AtomicU32::new(0);

    let fractional_pass_count = AtomicF64::new(0.0);

    let masonry_fail_count = AtomicU32::new(0);
    let subgrid_fail_count = AtomicU32::new(0);
    let writing_mode_fail_count = AtomicU32::new(0);
    let direction_fail_count = AtomicU32::new(0);
    let float_fail_count = AtomicU32::new(0);
    let calc_fail_count = AtomicU32::new(0);
    let intrinsic_size_fail_count = AtomicU32::new(0);
    let script_fail_count = AtomicU32::new(0);
    let other_fail_count = AtomicU32::new(0);
    let start = Instant::now();
    let start_timestamp = unix_timestamp();

    let num = AtomicU32::new(0);

    let base_font_context = parley::FontContext::default();

    let thread_state: ThreadLocal<RefCell<ThreadCtx>> = ThreadLocal::new();

    let mut results: Vec<TestResult> = test_paths
        .into_par_iter()
        .map(|path| {
            let mut ctx = thread_state
                .get_or(|| {
                    let renderer = VelloImageRenderer::new(WIDTH, HEIGHT);
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
                    let reftest_re =
                        Regex::new(r#"<link\s+rel=['"]?match['"]?\s+href=['"]([^'"]+)['"]"#)
                            .unwrap();

                    let float_re = Regex::new(r#"float:"#).unwrap();
                    let intrinsic_re =
                        Regex::new(r#"(width|height): ?(min|max|fit)-content"#).unwrap();
                    let calc_re = Regex::new(r#"calc\("#).unwrap();
                    let direction_re = Regex::new(r#"direction:|directionRTL"#).unwrap();
                    let writing_mode_re = Regex::new(r#"writing-mode:|vertical(RL|LR)"#).unwrap();
                    let subgrid_re = Regex::new(r#"subgrid"#).unwrap();
                    let masonry_re = Regex::new(r#"masonry"#).unwrap();
                    let script_re = Regex::new(r#"<script|onload="#).unwrap();

                    let attrtest_re =
                        Regex::new(r#"checkLayout\(\s*['"]([^'"]*)['"]\s*(,\s*(true|false))?\)"#)
                            .unwrap();

                    let dummy_base_url = Url::parse("http://dummy.local").unwrap();
                    let navigation_provider = Arc::new(DummyNavigationProvider);

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
                        float_re,
                        intrinsic_re,
                        calc_re,
                        direction_re,
                        writing_mode_re,
                        subgrid_re,
                        masonry_re,
                        script_re,
                        out_dir: out_dir.clone(),
                        wpt_dir: wpt_dir.clone(),
                        dummy_base_url,
                        navigation_provider,
                    })
                })
                .borrow_mut();

            // Clear any pending requests to avoid failed requests from a previous test interfering with subsequent tests
            ctx.net_provider.reset();

            let num = num.fetch_add(1, Ordering::Relaxed) + 1;

            let relative_path = path
                .strip_prefix(&ctx.wpt_dir)
                .unwrap()
                .to_string_lossy()
                .replace("\\", "/");

            let start = Instant::now();

            let result = catch_unwind(AssertUnwindSafe(|| {
                panic_backtrace::backtrace_cutoff(|| process_test_file(&mut ctx, &relative_path))
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

            // Bump counts
            match status {
                TestStatus::Pass => pass_count.fetch_add(1, Ordering::Relaxed),
                TestStatus::Fail => {
                    if flags.contains(TestFlags::USES_MASONRY) {
                        masonry_fail_count.fetch_add(1, Ordering::Relaxed);
                    } else if flags.contains(TestFlags::USES_SUBGRID) {
                        subgrid_fail_count.fetch_add(1, Ordering::Relaxed);
                    } else if flags.contains(TestFlags::USES_WRITING_MODE) {
                        writing_mode_fail_count.fetch_add(1, Ordering::Relaxed);
                    } else if flags.contains(TestFlags::USES_DIRECTION) {
                        direction_fail_count.fetch_add(1, Ordering::Relaxed);
                    } else if flags.contains(TestFlags::USES_INTRINSIC_SIZE) {
                        intrinsic_size_fail_count.fetch_add(1, Ordering::Relaxed);
                    } else if flags.contains(TestFlags::USES_CALC) {
                        calc_fail_count.fetch_add(1, Ordering::Relaxed);
                    } else if flags.contains(TestFlags::USES_FLOAT) {
                        float_fail_count.fetch_add(1, Ordering::Relaxed);
                    } else if kind == TestKind::Ref && flags.contains(TestFlags::USES_SCRIPT) {
                        script_fail_count.fetch_add(1, Ordering::Relaxed);
                    } else {
                        other_fail_count.fetch_add(1, Ordering::Relaxed);
                    }
                    fail_count.fetch_add(1, Ordering::Relaxed)
                }
                TestStatus::Skip => skip_count.fetch_add(1, Ordering::Relaxed),
                TestStatus::Crash => crash_count.fetch_add(1, Ordering::Relaxed),
            };

            // Bump fractional count
            fractional_pass_count.fetch_add(subtest_counts.pass_fraction(), Ordering::Relaxed);

            // Bump subtest counts
            subtest_count.fetch_add(subtest_counts.total, Ordering::Relaxed);
            subtest_pass_count.fetch_add(subtest_counts.pass, Ordering::Relaxed);
            subtest_fail_count.fetch_add(
                subtest_counts.total - subtest_counts.pass,
                Ordering::Relaxed,
            );

            let result = TestResult {
                name: relative_path,
                kind,
                flags,
                status,
                subtest_counts,
                subtest_results,
                duration: start.elapsed(),
                panic_info,
            };

            // Print status line
            let mut out = stdout().lock();
            write!(out, "[{num}/{count}] ").unwrap();
            result.print_to(out);

            result
        })
        .collect();

    let end_timestamp = unix_timestamp();

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
    let count = count as u32;

    let fractional_pass_count = fractional_pass_count.load(Ordering::SeqCst);
    let subtest_count = subtest_count.load(Ordering::SeqCst);
    let subtest_pass_count = subtest_pass_count.load(Ordering::SeqCst);

    let subgrid_fail_count = subgrid_fail_count.load(Ordering::SeqCst);
    let masonry_fail_count = masonry_fail_count.load(Ordering::SeqCst);
    let writing_mode_fail_count = writing_mode_fail_count.load(Ordering::SeqCst);
    let direction_fail_count = direction_fail_count.load(Ordering::SeqCst);
    let float_fail_count = float_fail_count.load(Ordering::SeqCst);
    let calc_fail_count = calc_fail_count.load(Ordering::SeqCst);
    let intrinsic_size_fail_count = intrinsic_size_fail_count.load(Ordering::SeqCst);
    let script_fail_count = script_fail_count.load(Ordering::SeqCst);
    let other_fail_count = other_fail_count.load(Ordering::SeqCst);

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
    let crash_percent_run = as_percent(crash_count, run_count);
    let crash_percent_total = as_percent(crash_count, count);

    let subtest_pass_percent = as_percent(subtest_pass_count, subtest_count);

    println!(
        "Done in {:.2}s",
        (Instant::now() - start).as_millis() as f64 / 1000.0
    );
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
    if masonry_fail_count > 0 {
        println!("{masonry_fail_count:>4} use masonry (M)");
    }

    // Generate wpt_expectations.txt
    let expectations = generate_expectations(&results);
    let expectations_path = out_dir.join("wpt_expectations.txt");
    fs::write(&expectations_path, expectations).unwrap();

    // Generate wptreport.json
    let report_start = Instant::now();
    let report = generate_report(&wpt_dir, results, start_timestamp, end_timestamp);
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
