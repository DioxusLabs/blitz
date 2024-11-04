use blitz_dom::net::Resource;
use blitz_dom::{HtmlDocument, Viewport};
use blitz_net::{MpscCallback, Provider};
use blitz_renderer_vello::render_to_buffer;
use blitz_traits::net::SharedProvider;

use regex::Regex;

use tokio::runtime::Handle;

use dify::diff::RunParams;
use image::{ImageBuffer, ImageFormat, RgbaImage};
use log::{error, info};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{env, fs};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::timeout;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const SCALE: u32 = 2;

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
    let viewport = Viewport::new(WIDTH * SCALE, HEIGHT * SCALE, SCALE as f32);

    let (mut receiver, callback) = MpscCallback::new();
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

    let reference_file_re = Regex::new(r#"<link\s+rel="match"\s+href="([^"]+)""#)
        .expect("Failed to compile regex for match re");

    let wpt_dir = env::var("WPT_DIR").expect("WPT_DIR is not set");
    info!("WPT_DIR: {}", wpt_dir);

    let cargo_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = cargo_dir.join("examples").join("output").join("wpt");

    let wpt_dir = Path::new(&wpt_dir);
    if !wpt_dir.exists() {
        error!("WPT_DIR does not exist. This should be set to a local copy of https://github.com/web-platform-tests/wpt.");
    }

    let test_paths = collect_tests(&wpt_dir);

    let mut blitz_context = rt.block_on(async { setup_blitz() });

    for path in test_paths {
        rt.block_on(async {
            process_test_file(
                &path,
                &reference_file_re,
                &wpt_dir,
                &mut blitz_context,
                &out_dir,
            )
            .await;
        });
    }
}

async fn process_test_file(
    path: &PathBuf,
    reference_file_re: &Regex,
    wpt_dir: &Path,
    blitz_context: &mut BlitzContext,
    out_dir: &PathBuf,
) {
    info!("Processing test file: {}", path.display());

    let file_contents = std::fs::read_to_string(path).expect("Could not read file.");

    let reference = reference_file_re
        .captures(file_contents.as_str())
        .and_then(|captures| captures.get(1).map(|href| href.as_str().to_string()));

    if let Some(reference) = reference {
        process_test_file_with_ref(
            path,
            file_contents.as_str(),
            reference.as_str(),
            wpt_dir,
            blitz_context,
            &out_dir,
        )
        .await;
    } else {
        info!(
            "Skipping test file: {}. No reference found.",
            path.display()
        );

        // Todo: Handle other test formats.
    }
}

async fn process_test_file_with_ref(
    actual_path: &PathBuf,
    actual_file_contents: &str,
    ref_file: &str,
    wpt_dir: &Path,
    blitz_context: &mut BlitzContext,
    out_dir: &PathBuf,
) {
    if !ref_file.ends_with(".html") {
        return;
    }
    let ref_file: PathBuf = if ref_file.starts_with("/") {
        wpt_dir.to_path_buf().join(&ref_file[1..])
    } else {
        actual_path.parent().unwrap().to_path_buf().join(ref_file)
    };

    let reference_file_contents = fs::read_to_string(&ref_file);

    if reference_file_contents.is_err() {
        error!(
            "Skipping test file: {}. Reference file missing {}",
            actual_path.display(),
            ref_file.display()
        );
        panic!(
            "Skipping test file: {}. No reference found.",
            ref_file.display()
        );
    }

    let reference_file_contents = reference_file_contents.unwrap();

    // Resolve .. and such and remove any canonical syntax that we don't care about.
    let actual_base_url = fs::canonicalize(actual_path)
        .unwrap()
        .display()
        .to_string()
        .replace("\\\\?\\", "")
        .replace("\\", "/");

    let ref_base_url = fs::canonicalize(&ref_file)
        .unwrap()
        .display()
        .to_string()
        .replace("\\\\?\\", "")
        .replace("\\", "/");

    let (actual_buffer, actual_width, actual_height) = {
        let mut document = HtmlDocument::from_html(
            &actual_file_contents,
            Some(format!("file://{}", actual_base_url)),
            Vec::new(),
            Arc::clone(&blitz_context.net) as SharedProvider<Resource>,
        );

        document
            .as_mut()
            .set_viewport(blitz_context.viewport.clone());

        while !blitz_context.net.is_empty() {
            let Ok(Some(res)) =
                timeout(Duration::from_secs(5), blitz_context.receiver.recv()).await
            else {
                break;
            };
            document.as_mut().load_resource(res);
        }

        // Compute style, layout, etc for HtmlDocument
        document.as_mut().resolve();

        // Determine height to render
        let computed_height = document.as_ref().root_element().final_layout.size.height;
        let render_height = (computed_height as u32).max(HEIGHT).min(4000);

        // Render document to RGBA buffer
        let buffer = render_to_buffer(
            document.as_ref(),
            Viewport::new(WIDTH * SCALE, render_height * SCALE, SCALE as f32),
        )
        .await;

        let name = actual_path
            .strip_prefix(wpt_dir)
            .unwrap()
            .display()
            .to_string()
            .replace("/", "_")
            .replace("\\", "_")
            + "-actual.png";
        let mut file = File::create(&out_dir.join(name)).unwrap();
        write_png(&mut file, &buffer, WIDTH * SCALE, render_height * SCALE);
        (buffer, WIDTH * SCALE, render_height * SCALE)
    };

    let (ref_buffer, ref_width, ref_height) = {
        let mut document = HtmlDocument::from_html(
            &reference_file_contents,
            Some(format!("file://{}", ref_base_url)),
            Vec::new(),
            Arc::clone(&blitz_context.net) as SharedProvider<Resource>,
        );

        document
            .as_mut()
            .set_viewport(blitz_context.viewport.clone());

        while !blitz_context.net.is_empty() {
            let Ok(Some(res)) =
                timeout(Duration::from_secs(5), blitz_context.receiver.recv()).await
            else {
                break;
            };
            document.as_mut().load_resource(res);
        }

        // Compute style, layout, etc for HtmlDocument
        document.as_mut().resolve();

        // Determine height to render
        let computed_height = document.as_ref().root_element().final_layout.size.height;
        let render_height = (computed_height as u32).max(HEIGHT).min(4000);

        // Render document to RGBA buffer
        let buffer = render_to_buffer(
            document.as_ref(),
            Viewport::new(WIDTH * SCALE, render_height * SCALE, SCALE as f32),
        )
        .await;

        let name = ref_file
            .strip_prefix(wpt_dir)
            .unwrap()
            .display()
            .to_string()
            .replace("/", "_")
            .replace("\\", "_")
            + "-reference.png";

        let mut file = File::create(&out_dir.join(name)).unwrap();
        write_png(&mut file, &buffer, WIDTH * SCALE, render_height * SCALE);
        (buffer, WIDTH * SCALE, render_height * SCALE)
    };

    let actual_image = ImageBuffer::from_raw(actual_width, actual_height, actual_buffer).unwrap();
    let ref_image = ImageBuffer::from_raw(ref_width, ref_height, ref_buffer).unwrap();

    let x = None;
    let y = None;
    let diff = dify::diff::get_results(actual_image, ref_image, 0.1f32, true, None, &x, &y);

    if let Some(diff) = diff {
        let name = actual_path
            .strip_prefix(wpt_dir)
            .unwrap()
            .display()
            .to_string()
            .replace("/", "_")
            .replace("\\", "_")
            + "-diff.png";
        diff.1
            .save_with_format(&out_dir.join(name), ImageFormat::Png)
            .unwrap();
        error!("FAIL: {}", actual_path.display());
    } else {
        info!("PASS: {}", actual_path.display());
    }
}

fn path_contains_directory(path: &PathBuf, dir_name: &str) -> bool {
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
    writer.write_image_data(&buffer).unwrap();
    writer.finish().unwrap();
}
