//! Load first CLI argument as a url. Fallback to google.com if no CLI argument is provided.

use blitz_headless::{HeadlessDocument, HeadlessOptions};
use blitz_net::Provider;
use reqwest::Url;
use std::sync::Arc;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

#[tokio::main]
async fn main() {
    let mut timer = Timer::init();

    let url_string = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://www.google.com".into());

    println!("{}", url_string);

    // Assert that url is valid
    let url = Url::parse(&url_string)
        .unwrap_or_else(|_| Url::parse(&format!("https://{url_string}")).expect("Invalid url"));
    let url_string = url.to_string();

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
    let scale: f64 = 2.0;
    let height: u32 = 800;
    let width: u32 = std::env::args()
        .nth(2)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(1200);

    let net = Arc::new(Provider::new(None));

    timer.time("Setup document prerequisites");

    // Create document
    let mut document = HeadlessDocument::from_html_with(
        &html,
        HeadlessOptions {
            width: width * (scale as u32),
            height: height * (scale as u32),
            scale: scale as f32,
            base_url: Some(url_string.clone()),
            net_provider: Some(Arc::clone(&net) as _),
            ..Default::default()
        },
    );

    timer.time("Parsed document");

    // Resolve style/layout, waiting for sub-resources (stylesheets, images, fonts) to load
    document.resolve_until_network_idle();

    timer.time("Fetched assets and resolved styles and layout");

    // Determine height to render: the content height, clamped between the viewport
    // height and 4000px
    let render_width = (width as f64 * scale) as u32;
    let render_height = (document.content_height() as f64)
        .max(height as f64 * scale)
        .min(4000.0 * scale) as u32;

    // Render document to RGBA screenshot
    let screenshot = document.screenshot_with_size(render_width, render_height);

    timer.time("Rendered to buffer");

    // Determine output path and write PNG to it. TODO: make configurable.
    let out_path = compute_filename(&url_string);
    screenshot.save_png(&out_path);

    timer.time("Wrote out png");

    // Log result.
    timer.total_time("\nDone");
    println!("Screenshot is ({width}x{render_height})");
    println!("Written to {}", out_path.display());
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
