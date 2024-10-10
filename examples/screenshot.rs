//! Load first CLI argument as a url. Fallback to google.com if no CLI argument is provided.

use blitz_dom::net::Resource;
use blitz_dom::{HtmlDocument, Viewport};
use blitz_net::{MpscCallback, Provider};
use blitz_renderer_vello::render_to_buffer;
use blitz_traits::net::{set_provider, SharedCallback};
use reqwest::Url;
use std::sync::Arc;
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};
use tokio::runtime::Handle;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

#[tokio::main]
async fn main() {
    let mut timer = Timer::init();

    let url = std::env::args()
        .skip(1)
        .next()
        .unwrap_or_else(|| "https://www.google.com".into());

    println!("{}", url);

    // Assert that url is valid
    let url = url.to_owned();
    Url::parse(&url).expect("Invalid url");

    // Fetch HTML from URL
    let html = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .call()
        .unwrap()
        .into_string()
        .unwrap();

    timer.time("Fetched HTML");

    // Setup viewport. TODO: make configurable.
    let scale = 2;
    let height = 800;
    let width: u32 = std::env::args()
        .skip(2)
        .next()
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(1200);

    let (mut recv, callback) = MpscCallback::new();
    let callback = Arc::new(callback);
    let net = Provider::new(Handle::current());
    set_provider(net);

    timer.time("Setup document prerequisites");

    // Create HtmlDocument
    let mut document = HtmlDocument::from_html(
        &html,
        Some(url.clone()),
        Vec::new(),
        Arc::clone(&callback) as SharedCallback<Resource>,
    );

    timer.time("Parsed document");

    document
        .as_mut()
        .set_viewport(Viewport::new(width * scale, height * scale, scale as f32));

    while let Some(res) = recv.recv().await {
        document.as_mut().load_resource(res);
        if Arc::strong_count(&callback) == 2 {
            break;
        }
    }

    timer.time("Fetched assets");

    // Compute style, layout, etc for HtmlDocument
    document.as_mut().resolve();

    timer.time("Resolved styles and layout");

    // Determine height to render
    let computed_height = document.as_ref().root_element().final_layout.size.height;
    let render_height = (computed_height as u32).max(height).min(4000);

    // Render document to RGBA buffer
    let buffer = render_to_buffer(
        document.as_ref(),
        Viewport::new(width * scale, render_height * scale, scale as f32),
    )
    .await;

    timer.time("Rendered to buffer");

    // Determine output path, and open a file at that path. TODO: make configurable.
    let out_path = compute_filename(&url);
    let mut file = File::create(&out_path).unwrap();

    // Encode buffer as PNG and write it to a file
    write_png(&mut file, &buffer, width * scale, render_height * scale);

    timer.time("Wrote out png");

    // Log result.
    timer.total_time("\nDone");
    println!("Screenshot is ({width}x{render_height})");
    println!("Written to {}", out_path.display());
}

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

fn compute_filename(url: &str) -> PathBuf {
    let cargo_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = cargo_dir.join("examples/output");

    let url = url.strip_prefix("https://").unwrap_or(url);
    let url = url.strip_prefix("http://").unwrap_or(url);
    let url_sanitized: String = url
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(12)
        .collect();

    out_dir.join(&url_sanitized).with_extension("png")
}

struct Timer {
    initial_time: Instant,
    last_time: Instant,
}

impl Timer {
    fn init() -> Self {
        let time = Instant::now();
        Self {
            initial_time: time,
            last_time: time,
        }
    }

    fn time(&mut self, message: &str) {
        let now = Instant::now();
        let diff = (now - self.last_time).as_millis();
        println!("{message} in {diff}ms");

        self.last_time = now;
    }

    fn total_time(&mut self, message: &str) {
        let now = Instant::now();
        let diff = (now - self.initial_time).as_millis();
        println!("{message} in {diff}ms");

        self.last_time = now;
    }
}
