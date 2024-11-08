use attr_test::process_attr_test;
use blitz_dom::net::Resource;
use blitz_dom::Viewport;
use blitz_net::{MpscCallback, Provider};
use blitz_renderer_vello::VelloImageRenderer;
use parley::FontContext;
use reqwest::Url;
use thread_local::ThreadLocal;
use tower_http::services::ServeDir;

use regex::Regex;

use rayon::prelude::*;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedReceiver;

use axum::Router;
use log::{error, info};
use owo_colors::OwoColorize;
use std::cell::RefCell;
use std::fmt::Display;
use std::net::SocketAddr;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use std::{env, fs};

mod attr_test;
mod ref_test;

use ref_test::process_ref_test;

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const SCALE: f64 = 1.0;

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

enum TestResult {
    Pass,
    Fail,
    Skip,
}

const BLOCKED_TESTS: &[&str] = &[
    // This test currently causes a wgpu validation error:
    // "Buffer size 17179869184 is greater than the maximum buffer size"
    "css/css-flexbox/flexbox-paint-ordering-002.xhtml",
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
                        || path_str.ends_with("-ref.xhtml")
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

struct BlitzContext {
    receiver: UnboundedReceiver<Resource>,
    viewport: Viewport,
    net: Arc<Provider<Resource>>,
}

fn setup_blitz() -> BlitzContext {
    let viewport = Viewport::new(
        (WIDTH as f64 * SCALE).floor() as u32,
        (HEIGHT as f64 * SCALE).floor() as u32,
        SCALE as f32,
    );

    let (receiver, callback) = MpscCallback::new();
    let callback = Arc::new(callback);
    let net = Arc::new(Provider::new(Handle::current(), callback));

    BlitzContext {
        receiver,
        viewport,
        net,
    }
}

fn clone_font_ctx(ctx: &FontContext) -> FontContext {
    FontContext {
        collection: ctx.collection.clone(),
        source_cache: ctx.source_cache.clone(),
    }
}

struct ThreadState {
    renderer: VelloImageRenderer,
    font_ctx: FontContext,
    test_buffer: Vec<u8>,
    ref_buffer: Vec<u8>,
}

fn main() {
    env_logger::init();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    let _guard = rt.enter();

    let reftest_re = Regex::new(r#"<link\s+rel="match"\s+href="([^"]+)""#)
        .expect("Failed to compile reftest regex");

    let attrtest_re = Regex::new(r#"checkLayout\(\s*['"]([^'"]*)['"]\s*(,\s*(true|false))?\)"#)
        .expect("Failed to compile attrtest regex");

    let wpt_dir = env::var("WPT_DIR").expect("WPT_DIR is not set");
    info!("WPT_DIR: {}", wpt_dir);
    let wpt_dir2 = wpt_dir.clone();
    let wpt_dir = Path::new(wpt_dir.as_str());
    if !wpt_dir.exists() {
        error!("WPT_DIR does not exist. This should be set to a local copy of https://github.com/web-platform-tests/wpt.");
    }
    let test_paths = collect_tests(wpt_dir);
    let count = test_paths.len();

    let cargo_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = cargo_dir.join("output");
    if fs::exists(&out_dir).unwrap() {
        fs::remove_dir_all(&out_dir).unwrap();
    }
    fs::create_dir(&out_dir).unwrap();

    rt.spawn(async move {
        let router = Router::new().nest_service("/", ServeDir::new(&wpt_dir2));
        serve(router, 3000).await;
    });

    let base_url = Url::parse("http://localhost:3000").unwrap();

    let pass_count = AtomicU32::new(0);
    let fail_count = AtomicU32::new(0);
    let skip_count = AtomicU32::new(0);
    let crash_count = AtomicU32::new(0);
    let start = Instant::now();

    let num = AtomicU32::new(0);

    let base_font_context = parley::FontContext::default();

    let thread_state: ThreadLocal<RefCell<ThreadState>> = ThreadLocal::new();

    test_paths.into_par_iter().for_each_init(
        || rt.enter(),
        |_guard, path| {
            let mut state = thread_state
                .get_or(|| {
                    let renderer = rt.block_on(VelloImageRenderer::new(WIDTH, HEIGHT, SCALE));
                    let font_ctx = clone_font_ctx(&base_font_context);
                    let test_buffer = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
                    let ref_buffer = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);

                    RefCell::new(ThreadState {
                        renderer,
                        font_ctx,
                        test_buffer,
                        ref_buffer,
                    })
                })
                .borrow_mut();
            let state = &mut *state;

            let num = num.fetch_add(1, Ordering::SeqCst) + 1;

            let relative_path = path
                .strip_prefix(wpt_dir)
                .unwrap()
                .display()
                .to_string()
                .replace("\\", "/");

            let url = base_url.join(relative_path.as_str()).unwrap();

            let renderer = &mut state.renderer;
            let font_ctx = &state.font_ctx;
            let test_buffer = &mut state.test_buffer;
            let ref_buffer = &mut state.ref_buffer;

            let result = catch_unwind(AssertUnwindSafe(|| {
                let mut blitz_context = setup_blitz();
                let (kind, result) = rt.block_on(process_test_file(
                    renderer,
                    font_ctx,
                    test_buffer,
                    ref_buffer,
                    &url,
                    &reftest_re,
                    &attrtest_re,
                    &base_url,
                    &mut blitz_context,
                    &out_dir,
                ));
                match result {
                    TestResult::Pass => {
                        pass_count.fetch_add(1, Ordering::SeqCst);
                        println!(
                            "[{}/{}] {} {} ({})",
                            num,
                            count,
                            "PASS".green(),
                            &relative_path,
                            kind,
                        );
                    }
                    TestResult::Fail => {
                        fail_count.fetch_add(1, Ordering::SeqCst);
                        println!(
                            "[{}/{}] {} {} ({})",
                            num,
                            count,
                            "FAIL".red(),
                            &relative_path,
                            kind,
                        );
                    }
                    TestResult::Skip => {
                        skip_count.fetch_add(1, Ordering::SeqCst);
                        println!(
                            "[{}/{}] {} {} ({})",
                            num,
                            count,
                            "SKIP".blue(),
                            &relative_path,
                            kind,
                        );
                    }
                }
            }));

            if result.is_err() {
                crash_count.fetch_add(1, Ordering::SeqCst);
                println!(
                    "[{}/{}] {} {} ({})",
                    num,
                    count,
                    "CRASH".red(),
                    relative_path,
                    TestKind::Unknown,
                );
            }
        },
    );

    let pass_count = pass_count.load(Ordering::SeqCst);
    let fail_count = fail_count.load(Ordering::SeqCst);
    let crash_count = crash_count.load(Ordering::SeqCst);
    let skip_count = skip_count.load(Ordering::SeqCst);

    let run_count = pass_count + fail_count + crash_count;

    println!("---");
    println!("Done in {}s", (Instant::now() - start).as_secs());
    println!("{pass_count} tests PASSED.");
    println!("{fail_count} tests FAILED.");
    println!("{crash_count} tests CRASHED.");
    println!("{skip_count} tests SKIPPED.");
    println!("---");
    let pessimistic_percent = (pass_count as f32 / count as f32) * 100.0;
    let optimistic_percent = (pass_count as f32 / run_count as f32) * 100.0;
    println!("Percent of total: {pessimistic_percent:.2}%");
    println!("Percent of run: {optimistic_percent:.2}%");
}

async fn serve(app: Router, port: u16) {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .await
        .expect("Failed to create http server.");
}

#[allow(clippy::too_many_arguments)]
async fn process_test_file(
    renderer: &mut VelloImageRenderer,
    font_ctx: &FontContext,
    test_buffer: &mut Vec<u8>,
    ref_buffer: &mut Vec<u8>,
    path: &Url,
    reftest_re: &Regex,
    attrtest_re: &Regex,
    base_url: &Url,
    blitz_context: &mut BlitzContext,
    out_dir: &Path,
) -> (TestKind, TestResult) {
    info!("Processing test file: {}", path);

    let file_contents = reqwest::get(path.clone())
        .await
        .expect("Could not read file.")
        .text()
        .await
        .unwrap();

    // Ref Test
    let reference = reftest_re
        .captures(&file_contents)
        .and_then(|captures| captures.get(1).map(|href| href.as_str().to_string()));
    if let Some(reference) = reference {
        let result = process_ref_test(
            renderer,
            font_ctx,
            test_buffer,
            ref_buffer,
            path,
            file_contents.as_str(),
            reference.as_str(),
            base_url,
            blitz_context,
            out_dir,
        )
        .await;

        return (TestKind::Ref, result);
    }

    // Attr Test
    let mut matches = attrtest_re.captures_iter(&file_contents);
    let first = matches.next();
    let second = matches.next();
    if first.is_some() && second.is_none() {
        // TODO: handle tests with multiple calls to checkLayout.
        let captures = first.unwrap();
        let selector = captures.get(1).unwrap().as_str().to_string();

        let result = process_attr_test(
            font_ctx,
            path,
            &selector,
            &file_contents,
            base_url,
            blitz_context,
        )
        .await;

        return (TestKind::Attr, result);
    }

    // TODO: Handle other test formats.
    (TestKind::Unknown, TestResult::Skip)
}
