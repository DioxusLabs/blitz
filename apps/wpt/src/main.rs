use blitz_dom::net::Resource;
use blitz_dom::{HtmlDocument, Viewport};
use blitz_net::{MpscCallback, Provider};
use blitz_renderer_vello::VelloImageRenderer;
use blitz_traits::net::SharedProvider;
use reqwest::Url;
use tower_http::services::ServeDir;

use regex::Regex;

use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::timeout;

use axum::Router;
use image::{ImageBuffer, ImageFormat};
use log::{error, info};
use owo_colors::OwoColorize;
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

fn collect_tests(wpt_dir: &Path) -> Vec<PathBuf> {
    let pattern = format!(
        "{}/css/css-flexbox/**/*.html",
        wpt_dir.display().to_string().as_str()
    );
    let glob_results = glob::glob(&pattern).expect("Invalid glob pattern.");

    glob_results
        .filter_map(|glob_result| {
            if let Ok(path_buf) = glob_result {
                let is_tentative = path_buf.ends_with("tentative.html");
                let is_ref = path_buf.ends_with("-ref.html")
                    || path_contains_directory(&path_buf, "reference");
                let is_support_file = path_contains_directory(&path_buf, "support");

                if !is_tentative && !is_ref && !is_support_file {
                    Some(path_buf)
                } else {
                    None
                }
            } else {
                error!("Failure during glob.");
                panic!("Failure during glob");
            }
        })
        .collect()
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

    let mut renderer = rt.block_on(VelloImageRenderer::new(WIDTH, HEIGHT, SCALE));
    let mut test_buffer = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    let mut ref_buffer = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);

    let base_url = Url::parse("http://localhost:3000").unwrap();

    let pass_count = AtomicU32::new(0);
    let fail_count = AtomicU32::new(0);
    let skip_count = AtomicU32::new(0);
    let crash_count = AtomicU32::new(0);
    let start = Instant::now();

    for (index, path) in test_paths.into_iter().enumerate() {
        let num = index + 1;

        let relative_path = path
            .strip_prefix(wpt_dir)
            .unwrap()
            .display()
            .to_string()
            .replace("\\", "/");

        let url = base_url.join(relative_path.as_str()).unwrap();

        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut blitz_context = setup_blitz();
            let result = rt.block_on(process_test_file(
                &mut renderer,
                &mut test_buffer,
                &mut ref_buffer,
                &url,
                &reference_file_re,
                &base_url,
                &mut blitz_context,
                &out_dir,
            ));
            match result {
                TestResult::Pass => {
                    pass_count.fetch_add(1, Ordering::SeqCst);
                    println!("[{}/{}] {}: {}", num, count, "PASS".green(), &url);
                }
                TestResult::Fail => {
                    fail_count.fetch_add(1, Ordering::SeqCst);
                    println!("[{}/{}] {}: {}", num, count, "FAIL".red(), &url);
                }
                TestResult::Skip => {
                    skip_count.fetch_add(1, Ordering::SeqCst);
                    println!("[{}/{}] {}: {}", num, count, "SKIP".blue(), &url);
                }
            }
        }));

        if result.is_err() {
            crash_count.fetch_add(1, Ordering::SeqCst);
            println!("[{}/{}] {}: {}", num, count, "CRASH".red(), url);
        }
    }

    println!("---");
    println!("Done in {}s", (Instant::now() - start).as_secs());
    println!("{} tests PASSED.", pass_count.load(Ordering::SeqCst));
    println!("{} tests FAILED.", fail_count.load(Ordering::SeqCst));
    println!("{} tests CRASHED.", crash_count.load(Ordering::SeqCst));
    println!("{} tests SKIPPED.", skip_count.load(Ordering::SeqCst));
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

    {
        let mut document = HtmlDocument::from_html(
            test_file_contents,
            Some(test_url.to_string()),
            Vec::new(),
            Arc::clone(&blitz_context.net) as SharedProvider<Resource>,
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
        renderer.render_document(document.as_ref(), test_buffer);
        let path = format!("{}{}", test_base_url, "-test.png");

        let out_file = out_dir.join(path);
        fs::create_dir_all(out_file.parent().unwrap()).unwrap();
        let mut file = File::create(out_file).unwrap();
        write_png(&mut file, test_buffer, WIDTH, render_height);
    };

    {
        let mut document = HtmlDocument::from_html(
            &reference_file_contents,
            Some(ref_url.to_string()),
            Vec::new(),
            Arc::clone(&blitz_context.net) as SharedProvider<Resource>,
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
        renderer.render_document(document.as_ref(), ref_buffer);

        let path = format!("{}{}", test_base_url, "-ref.png");

        let out_file = out_dir.join(path);
        fs::create_dir_all(out_file.parent().unwrap()).unwrap();
        let mut file = File::create(out_file).unwrap();
        write_png(&mut file, ref_buffer, WIDTH, render_height);
    };

    let test_image = ImageBuffer::from_raw(WIDTH, HEIGHT, test_buffer.clone()).unwrap();
    let ref_image = ImageBuffer::from_raw(WIDTH, HEIGHT, ref_buffer.clone()).unwrap();

    let x = None;
    let y = None;
    let diff = dify::diff::get_results(test_image, ref_image, 0.1f32, true, None, &x, &y);

    if let Some(diff) = diff {
        let path = out_dir.join(format!("{}{}", test_base_url, "-diff.png"));
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        diff.1.save_with_format(path, ImageFormat::Png).unwrap();
        TestResult::Pass
    } else {
        TestResult::Fail
    }
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
