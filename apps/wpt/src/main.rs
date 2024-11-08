use blitz_dom::net::Resource;
use blitz_dom::{HtmlDocument, Viewport};
use blitz_net::{MpscCallback, Provider};
use blitz_renderer_vello::VelloImageRenderer;
use blitz_traits::net::SharedProvider;
use parley::FontContext;
use reqwest::Url;
use thread_local::ThreadLocal;
use tower_http::services::ServeDir;

use regex::Regex;

use rayon::prelude::*;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::timeout;

use axum::Router;
use image::{ImageBuffer, ImageFormat};
use log::{error, info};
use owo_colors::OwoColorize;
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::net::SocketAddr;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{env, fs};

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const SCALE: f64 = 1.0;

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

    let reference_file_re = Regex::new(r#"<link\s+rel="match"\s+href="([^"]+)""#)
        .expect("Failed to compile regex for match re");

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
                let result = rt.block_on(process_test_file(
                    renderer,
                    font_ctx,
                    test_buffer,
                    ref_buffer,
                    &url,
                    &reference_file_re,
                    &base_url,
                    &mut blitz_context,
                    &out_dir,
                ));
                match result {
                    TestResult::Pass => {
                        pass_count.fetch_add(1, Ordering::SeqCst);
                        println!("[{}/{}] {}: {}", num, count, "PASS".green(), &relative_path);
                    }
                    TestResult::Fail => {
                        fail_count.fetch_add(1, Ordering::SeqCst);
                        println!("[{}/{}] {}: {}", num, count, "FAIL".red(), &relative_path);
                    }
                    TestResult::Skip => {
                        skip_count.fetch_add(1, Ordering::SeqCst);
                        println!("[{}/{}] {}: {}", num, count, "SKIP".blue(), &relative_path);
                    }
                }
            }));

            if result.is_err() {
                crash_count.fetch_add(1, Ordering::SeqCst);
                println!("[{}/{}] {}: {}", num, count, "CRASH".red(), relative_path);
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
    reference_file_re: &Regex,
    base_url: &Url,
    blitz_context: &mut BlitzContext,
    out_dir: &Path,
) -> TestResult {
    info!("Processing test file: {}", path);

    let file_contents = reqwest::get(path.clone())
        .await
        .expect("Could not read file.")
        .text()
        .await
        .unwrap();

    let reference = reference_file_re
        .captures(file_contents.as_str())
        .and_then(|captures| captures.get(1).map(|href| href.as_str().to_string()));

    if let Some(reference) = reference {
        process_test_file_with_ref(
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
        .await
    } else {
        // Todo: Handle other test formats.
        TestResult::Skip
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_test_file_with_ref(
    renderer: &mut VelloImageRenderer,
    font_ctx: &FontContext,
    test_buffer: &mut Vec<u8>,
    ref_buffer: &mut Vec<u8>,
    test_url: &Url,
    test_file_contents: &str,
    ref_file: &str,
    base_url: &Url,
    blitz_context: &mut BlitzContext,
    out_dir: &Path,
) -> TestResult {
    let ref_url: Url = test_url.join(".").unwrap().join(ref_file).unwrap();

    let reference_file_contents = reqwest::get(ref_url.clone())
        .await
        .expect("Could not read file.")
        .text()
        .await;

    if reference_file_contents.is_err() {
        error!(
            "Skipping test file: {}. Reference file missing {}",
            test_url, ref_url
        );
        panic!("Skipping test file: {}. No reference found.", ref_url);
    }

    let reference_file_contents = reference_file_contents.unwrap();

    let test_base_url = test_url.to_string().replace(base_url.as_str(), "");
    let test_out_path = out_dir.join(format!("{}{}", test_base_url, "-test.png"));
    render_html_to_buffer(
        blitz_context,
        renderer,
        font_ctx,
        test_file_contents,
        test_url,
        test_buffer,
        &test_out_path,
    )
    .await;

    let ref_out_path = out_dir.join(format!("{}{}", test_base_url, "-ref.png"));
    render_html_to_buffer(
        blitz_context,
        renderer,
        font_ctx,
        &reference_file_contents,
        &ref_url,
        ref_buffer,
        &ref_out_path,
    )
    .await;

    if test_buffer == ref_buffer {
        return TestResult::Pass;
    }

    let test_image = ImageBuffer::from_raw(WIDTH, HEIGHT, test_buffer.clone()).unwrap();
    let ref_image = ImageBuffer::from_raw(WIDTH, HEIGHT, ref_buffer.clone()).unwrap();

    let diff = dify::diff::get_results(test_image, ref_image, 0.1f32, true, None, &None, &None);

    if let Some(diff) = diff {
        let path = out_dir.join(format!("{}{}", test_base_url, "-diff.png"));
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        diff.1.save_with_format(path, ImageFormat::Png).unwrap();
        TestResult::Fail
    } else {
        TestResult::Pass
    }
}

async fn render_html_to_buffer(
    blitz_context: &mut BlitzContext,
    renderer: &mut VelloImageRenderer,
    font_ctx: &FontContext,
    html: &str,
    base_url: &Url,
    buf: &mut Vec<u8>,
    out_path: &Path,
) {
    let mut document = HtmlDocument::from_html(
        html,
        Some(base_url.to_string()),
        Vec::new(),
        Arc::clone(&blitz_context.net) as SharedProvider<Resource>,
        Some(clone_font_ctx(font_ctx)),
    );

    document
        .as_mut()
        .set_viewport(blitz_context.viewport.clone());

    while !blitz_context.net.is_empty() {
        let Ok(Some(res)) =
            timeout(Duration::from_millis(500), blitz_context.receiver.recv()).await
        else {
            break;
        };
        document.as_mut().load_resource(res);
    }

    // Compute style, layout, etc for HtmlDocument
    document.as_mut().resolve();

    // Determine height to render
    // let computed_height = document.as_ref().root_element().final_layout.size.height;
    // let render_height = (computed_height as u32).clamp(HEIGHT, 4000);
    let render_height = HEIGHT;

    // Render document to RGBA buffer
    renderer.render_document(document.as_ref(), buf);

    fs::create_dir_all(out_path.parent().unwrap()).unwrap();
    let mut file = File::create(out_path).unwrap();
    write_png(&mut file, buf, WIDTH, render_height);
}

fn path_contains_directory(path: &Path, dir_name: &str) -> bool {
    path.components()
        .any(|component| component.as_os_str() == dir_name)
}

// Copied from screenshot.rs
fn write_png<W: Write>(writer: W, buffer: &[u8], width: u32, height: u32) {
    // Set pixels-per-meter. TODO: make configurable.
    const PPM: u32 = (144.0 * 39.3701) as u32;

    // Create PNG encoder
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: PPM,
        yppu: PPM,
        unit: png::Unit::Meter,
    }));

    // Write PNG data to writer
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(buffer).unwrap();
    writer.finish().unwrap();
}
