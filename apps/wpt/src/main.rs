use atomic_float::AtomicF64;
use blitz_dom::net::Resource;
use blitz_renderer_vello::VelloImageRenderer;
use blitz_traits::navigation::{DummyNavigationProvider, NavigationProvider};
use blitz_traits::{ColorScheme, Viewport};
use parley::FontContext;
use pollster::FutureExt as _;
use supports_hyperlinks::supports_hyperlinks;
use terminal_link::Link;
use thread_local::ThreadLocal;
use url::Url;

use rayon::prelude::*;
use regex::Regex;

use bitflags::bitflags;
use log::{error, info};
use owo_colors::OwoColorize;
use std::cell::RefCell;
use std::fmt::Display;
use std::io::{Write, stdout};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{self, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
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

bitflags! {
    pub struct TestFlags : u32 {
        const USES_FLOAT = 0b00000001;
        const USES_INTRINSIC_SIZE = 0b00000010;
        const USES_CALC = 0b00000100;
        const USES_DIRECTION = 0b00001000;
        const USES_WRITING_MODE = 0b00010000;
        const USES_SUBGRID = 0b00100000;
        const USES_MASONRY = 0b01000000;
    }
}

#[derive(Copy, Clone)]
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

fn collect_tests(wpt_dir: &Path) -> Vec<PathBuf> {
    let mut test_paths = Vec::new();

    let mut suites: Vec<_> = std::env::args().skip(1).collect();
    if suites.is_empty() {
        suites.push("css/css-flexbox".to_string());
        suites.push("css/css-grid".to_string());
    }

    for suite in suites {
        for pat in ["", "/**/*.htm", "/**/*.html", "/**/*.xht", "/**/*.xhtml"] {
            let pattern = format!("{}/{}{}", wpt_dir.display(), suite, pat);

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

                    let is_dir = path_buf.is_dir();

                    if is_ref | is_support_file | is_blocked | is_dir {
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
    duration: Duration,
    panic_msg: Option<String>,
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
        if !self.flags.is_empty() {
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

            write!(out, "{}", ")".bright_black()).unwrap();
        }

        // Newline
        writeln!(out).unwrap();

        if let Some(panic_msg) = &self.panic_msg {
            writeln!(out, "{}", panic_msg).unwrap();
        }
    }
}

fn main() {
    env_logger::init();

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
    let out_dir = cargo_dir.join("output");
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
    let other_fail_count = AtomicU32::new(0);
    let start = Instant::now();

    let num = AtomicU32::new(0);

    let base_font_context = parley::FontContext::default();

    let thread_state: ThreadLocal<RefCell<ThreadCtx>> = ThreadLocal::new();

    let mut results: Vec<TestResult> = test_paths
        .into_par_iter()
        .map(|path| {
            let mut ctx = thread_state
                .get_or(|| {
                    let renderer = VelloImageRenderer::new(WIDTH, HEIGHT, SCALE).block_on();
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
                        Regex::new(r#"<link\s+rel=['"]match['"]\s+href=['"]([^'"]+)['"]"#).unwrap();

                    let float_re = Regex::new(r#"float:"#).unwrap();
                    let intrinsic_re =
                        Regex::new(r#"(width|height): ?(min|max|fit)-content"#).unwrap();
                    let calc_re = Regex::new(r#"calc\("#).unwrap();
                    let direction_re = Regex::new(r#"direction:|directionRTL"#).unwrap();
                    let writing_mode_re = Regex::new(r#"writing-mode:|vertical(RL|LR)"#).unwrap();
                    let subgrid_re = Regex::new(r#"subgrid"#).unwrap();
                    let masonry_re = Regex::new(r#"masonry"#).unwrap();

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
                        out_dir: out_dir.clone(),
                        wpt_dir: wpt_dir.clone(),
                        dummy_base_url,
                        navigation_provider,
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
                process_test_file(&mut ctx, &relative_path).block_on()
            }));
            let (kind, flags, status, subtest_counts, panic_msg) = match result {
                Ok((kind, flags, subtest_counts)) => {
                    let status = subtest_counts.as_status();
                    (kind, flags, status, subtest_counts, None)
                }
                Err(err) => {
                    let str_msg = err.downcast_ref::<&str>().map(|s| s.to_string());
                    let string_msg = err.downcast_ref::<String>().map(|s| s.to_string());
                    let panic_msg = str_msg.or(string_msg);

                    (
                        TestKind::Unknown,
                        TestFlags::empty(),
                        TestStatus::Crash,
                        SubtestCounts::ZERO_OF_ZERO,
                        panic_msg,
                    )
                }
            };

            // Bump counts
            match status {
                TestStatus::Pass => pass_count.fetch_add(1, Ordering::SeqCst),
                TestStatus::Fail => {
                    if flags.contains(TestFlags::USES_MASONRY) {
                        masonry_fail_count.fetch_add(1, Ordering::SeqCst);
                    } else if flags.contains(TestFlags::USES_SUBGRID) {
                        subgrid_fail_count.fetch_add(1, Ordering::SeqCst);
                    } else if flags.contains(TestFlags::USES_WRITING_MODE) {
                        writing_mode_fail_count.fetch_add(1, Ordering::SeqCst);
                    } else if flags.contains(TestFlags::USES_DIRECTION) {
                        direction_fail_count.fetch_add(1, Ordering::SeqCst);
                    } else if flags.contains(TestFlags::USES_INTRINSIC_SIZE) {
                        intrinsic_size_fail_count.fetch_add(1, Ordering::SeqCst);
                    } else if flags.contains(TestFlags::USES_CALC) {
                        calc_fail_count.fetch_add(1, Ordering::SeqCst);
                    } else if flags.contains(TestFlags::USES_FLOAT) {
                        float_fail_count.fetch_add(1, Ordering::SeqCst);
                    } else {
                        other_fail_count.fetch_add(1, Ordering::SeqCst);
                    }
                    fail_count.fetch_add(1, Ordering::SeqCst)
                }
                TestStatus::Skip => skip_count.fetch_add(1, Ordering::SeqCst),
                TestStatus::Crash => crash_count.fetch_add(1, Ordering::SeqCst),
            };

            // Bump fractional count
            fractional_pass_count.fetch_add(subtest_counts.pass_fraction(), Ordering::SeqCst);

            // Bump subtest counts
            subtest_count.fetch_add(subtest_counts.total, Ordering::SeqCst);
            subtest_pass_count.fetch_add(subtest_counts.pass, Ordering::SeqCst);
            subtest_fail_count
                .fetch_add(subtest_counts.total - subtest_counts.pass, Ordering::SeqCst);

            let result = TestResult {
                name: relative_path,
                kind,
                flags,
                status,
                subtest_counts,
                duration: start.elapsed(),
                panic_msg,
            };

            // Print status line
            let mut out = stdout().lock();
            write!(out, "[{num}/{count}] ").unwrap();
            result.print_to(out);

            result
        })
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
    println!("{writing_mode_fail_count:>4} use writing-mode");
    println!("{direction_fail_count:>4} use direction");
    println!("{float_fail_count:>4} use floats");
    println!("{intrinsic_size_fail_count:>4} use intrinsic size keywords");
    println!("{calc_fail_count:>4} use calc");
    if subgrid_fail_count > 0 {
        println!("{subgrid_fail_count:>4} use subgrid");
    }
    if masonry_fail_count > 0 {
        println!("{masonry_fail_count:>4} use masonry");
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_test_file(
    ctx: &mut ThreadCtx,
    relative_path: &str,
) -> (TestKind, TestFlags, SubtestCounts) {
    info!("Processing test file: {}", relative_path);

    let file_contents = fs::read_to_string(ctx.wpt_dir.join(relative_path)).unwrap();

    // Compute flags
    let mut flags = TestFlags::empty();
    if ctx.float_re.is_match(&file_contents) {
        flags |= TestFlags::USES_FLOAT;
    }
    if ctx.intrinsic_re.is_match(&file_contents) {
        flags |= TestFlags::USES_INTRINSIC_SIZE;
    }
    if ctx.calc_re.is_match(&file_contents) {
        flags |= TestFlags::USES_CALC;
    }
    if ctx.direction_re.is_match(&file_contents) {
        flags |= TestFlags::USES_DIRECTION;
    }
    if ctx.writing_mode_re.is_match(&file_contents) {
        flags |= TestFlags::USES_WRITING_MODE;
    }
    if ctx.subgrid_re.is_match(&file_contents) {
        flags |= TestFlags::USES_SUBGRID;
    }
    if ctx.masonry_re.is_match(&file_contents) {
        flags |= TestFlags::USES_MASONRY;
    }

    // Ref Test
    let reference = ctx
        .reftest_re
        .captures(&file_contents)
        .and_then(|captures| captures.get(1).map(|href| href.as_str().to_string()));
    if let Some(reference) = reference {
        let results = process_ref_test(
            ctx,
            relative_path,
            file_contents.as_str(),
            reference.as_str(),
            &mut flags,
        )
        .block_on();

        return (TestKind::Ref, flags, results);
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

        println!("{}", selector);

        let results = process_attr_test(ctx, &selector, &file_contents, relative_path).block_on();

        return (TestKind::Attr, flags, results);
    }

    // TODO: Handle other test formats.
    (TestKind::Unknown, flags, SubtestCounts::ZERO_OF_ZERO)
}
