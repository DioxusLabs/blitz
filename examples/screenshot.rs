//! Load first CLI argument as a url. Fallback to google.com if no CLI argument is provided.

use anyrender::render_to_buffer;
use anyrender_vello::VelloImageRenderer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_net::{MpscCallback, Provider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use reqwest::Url;
use std::sync::Arc;
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

#[tokio::main]
async fn main() {
    let mut timer = Timer::init();

    let use_cpu_renderer = std::env::args().any(|arg| arg == "--cpu");

    let url_string = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://www.google.com".into());

    println!("{}", url_string);

    // Assert that url is valid
    let url = Url::parse(&url_string).expect("Invalid url");

    // Fetch HTML from URL
    let html = match url.scheme() {
        "file" => {
            let file_content = std::fs::read(url.path()).unwrap();
            String::from_utf8(file_content).unwrap()
        }
        _ => {
            let client = reqwest::Client::new();
            let response = client
                .get(url)
                .header("User-Agent", USER_AGENT)
                .send()
                .await
                .unwrap();
            response.text().await.unwrap()
        }
    };

    timer.time("Fetched HTML");

    // Setup viewport. TODO: make configurable.
    let scale = 2.0;
    let height = 800;
    let width: u32 = std::env::args()
        .nth(2)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(1200);

    let (_recv, callback) = MpscCallback::new();
    let callback = Arc::new(callback);
    let net = Arc::new(Provider::new(callback));

    timer.time("Setup document prerequisites");

    // Create HtmlDocument
    let mut document = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some(url_string.clone()),
            net_provider: Some(Arc::clone(&net) as _),
            ..Default::default()
        },
    );

    timer.time("Parsed document");

    document.as_mut().set_viewport(Viewport::new(
        width * (scale as u32),
        height * (scale as u32),
        scale as f32,
        ColorScheme::Light,
    ));
    document.resolve(0.0);

    while !net.is_empty() {
        document.resolve(0.0);

        // HACK: this fixes a deadlock by forcing thread synchronisation.
        println!("{} resources remaining {}", net.count(), net.is_empty());
    }

    timer.time("Fetched assets");

    // Compute style, layout, etc for HtmlDocument
    document.as_mut().resolve(0.0);

    timer.time("Resolved styles and layout");

    // Determine height to render
    let computed_height = document.as_ref().root_element().final_layout.size.height;
    let render_width = (width as f64 * scale) as u32;
    let render_height = ((computed_height as f64).max(height as f64).min(4000.0) * scale) as u32;

    // Render document to RGBA buffer
    let buffer = if use_cpu_renderer {
        render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| paint_scene(scene, document.as_ref(), scale, render_width, render_height),
            render_width,
            render_height,
        )
    } else {
        render_to_buffer::<VelloImageRenderer, _>(
            |scene| paint_scene(scene, document.as_ref(), scale, render_width, render_height),
            render_width,
            render_height,
        )
    };

    timer.time("Rendered to buffer");

    // Determine output path, and open a file at that path. TODO: make configurable.
    let out_path = compute_filename(&url_string);
    let mut file = File::create(&out_path).unwrap();

    // Encode buffer as PNG and write it to a file
    write_png(&mut file, &buffer, render_width, render_height);

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
    writer.write_image_data(buffer).unwrap();
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
