//! Load first CLI argument as a file: or http(s): URL, execute its scripts
//! with blitz-script, and render a screenshot to examples/output/.

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{Document as _, DocumentConfig, util::Color};
use blitz_net::Provider;
use blitz_paint::paint_scene;
use blitz_script::ScriptDocument;
use blitz_traits::shell::{ColorScheme, Viewport};
use peniko::Fill;
use peniko::kurbo::Rect;
use reqwest::Url;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Serves prefetched script sources from memory, falling back to the default
/// fetcher (`file:` and `data:` URLs)
struct PrefetchedScriptFetcher {
    scripts: HashMap<Url, String>,
}

impl blitz_script::ScriptFetcher for PrefetchedScriptFetcher {
    fn fetch(&self, url: &Url) -> Result<String, blitz_script::FetchError> {
        if let Some(source) = self.scripts.get(url) {
            return Ok(source.clone());
        }
        blitz_script::DefaultScriptFetcher.fetch(url)
    }
}

#[tokio::main]
async fn main() {
    let url_string = std::env::args().nth(1).expect("expected a file: URL");
    let url = Url::parse(&url_string).expect("Invalid url");
    let html = match url.scheme() {
        "file" => String::from_utf8(std::fs::read(url.path()).unwrap()).unwrap(),
        "http" | "https" => reqwest::get(url.clone())
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        scheme => panic!("unsupported URL scheme: {scheme}"),
    };

    let scale = 1.0;
    let height = 800;
    let width: u32 = std::env::args()
        .nth(2)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(1200);

    let net = Arc::new(Provider::new(None));

    let document = ScriptDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some(url_string.clone()),
            net_provider: Some(Arc::clone(&net) as _),
            viewport: Some(Viewport::new(
                width * (scale as u32),
                height * (scale as u32),
                scale as f32,
                ColorScheme::Light,
            )),
            ..Default::default()
        },
    );

    // Prefetch external http(s) scripts, since the `ScriptFetcher` API is
    // synchronous
    let mut scripts: HashMap<Url, String> = HashMap::new();
    for script_url in document.external_script_urls() {
        if matches!(script_url.scheme(), "http" | "https") && !scripts.contains_key(&script_url) {
            match reqwest::get(script_url.clone()).await {
                Ok(response) => {
                    scripts.insert(script_url, response.text().await.unwrap_or_default());
                }
                Err(err) => println!("Failed to fetch script {script_url}: {err}"),
            }
        }
    }
    let mut document = document.with_fetcher(PrefetchedScriptFetcher { scripts });

    document.execute_scripts();
    for error in document.take_js_errors() {
        println!("JS ERROR: {error}");
    }
    if let Ok(debug_js) = std::env::var("DEBUG_JS") {
        document.eval(&debug_js);
        for error in document.take_js_errors() {
            println!("DEBUG JS ERROR: {error}");
        }
    }

    // Pump the document: run due JS timers (e.g. jQuery's deferred ready
    // callback) and wait for in-flight network requests
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        document.poll(None);
        document.inner_mut().resolve(0.0);
        if net.is_empty() && document.next_timer_deadline().is_none_or(|t| t > deadline) {
            break;
        }
        if std::time::Instant::now() > deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    document.inner_mut().resolve(0.0);
    for error in document.take_js_errors() {
        println!("JS ERROR (timers): {error}");
    }

    let computed_height = document.inner().root_element().final_layout().size.height;
    let render_width = (width as f64 * scale) as u32;
    let render_height = ((computed_height as f64).max(height as f64).min(4000.0) * scale) as u32;

    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(
                Fill::NonZero,
                Default::default(),
                Color::WHITE,
                Default::default(),
                &Rect::new(0.0, 0.0, render_width as f64, render_height as f64),
            );
            paint_scene(
                scene,
                &mut document.inner_mut(),
                scale,
                render_width,
                render_height,
                0,
                0,
            );
        },
        render_width,
        render_height,
    );

    let out_path = compute_filename(&url_string);
    let mut file = File::create(&out_path).unwrap();
    write_png(&mut file, &buffer, render_width, render_height);
    println!("Written to {}", out_path.display());
}

fn write_png<W: Write>(writer: W, buffer: &[u8], width: u32, height: u32) {
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(buffer).unwrap();
    writer.finish().unwrap();
}

fn compute_filename(url: &str) -> PathBuf {
    let cargo_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = cargo_dir.join("examples/output");
    let url_sanitized: String = url
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .rev()
        .take(20)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    out_dir.join(&url_sanitized).with_extension("png")
}
