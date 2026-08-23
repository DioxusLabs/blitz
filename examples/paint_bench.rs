//! Benchmark the paint phase (blitz-paint `paint_scene` -> vello Scene encoding).
//!
//! Usage: paint_bench <url> [width] [height] [scale] [iters]

use anyrender::PaintScene as _;
use anyrender_vello::VelloScenePainter;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use reqwest::Url;
use std::sync::Arc;
use std::time::Instant;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let url_string = args.next().unwrap_or_else(|| "https://servo.org".into());
    let width: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1366);
    let height: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(768);
    let scale: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(2.0);
    let iters: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(100);

    let url = Url::parse(&url_string)
        .unwrap_or_else(|_| Url::parse(&format!("https://{url_string}")).expect("Invalid url"));
    let url_string = url.to_string();

    // Fetch HTML
    let html = match url.scheme() {
        "file" => String::from_utf8(std::fs::read(url.path()).unwrap()).unwrap(),
        _ => {
            let client = reqwest::Client::new();
            client
                .get(url)
                .header("User-Agent", USER_AGENT)
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        }
    };

    let net = Arc::new(Provider::new(None));

    let mut document = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some(url_string.clone()),
            net_provider: Some(Arc::clone(&net) as _),
            viewport: Some(Viewport::new(
                (width as f64 * scale) as u32,
                (height as f64 * scale) as u32,
                scale as f32,
                ColorScheme::Light,
            )),
            ..Default::default()
        },
    );

    // Wait for assets
    loop {
        document.resolve(0.0);
        if net.is_empty() {
            break;
        }
    }
    document.as_mut().resolve(0.0);

    println!("Loaded {url_string} at {width}x{height}@{scale}x; running {iters} paint iterations");

    let render_width = (width as f64 * scale) as u32;
    let render_height = (height as f64 * scale) as u32;

    let mut scene = vello::Scene::new();
    let mut times_us: Vec<u128> = Vec::with_capacity(iters);

    // Warmup
    for _ in 0..5 {
        let mut painter = VelloScenePainter::new(&mut scene);
        painter.reset();
        paint_scene(
            &mut painter,
            document.as_mut(),
            scale,
            render_width,
            render_height,
            0,
            0,
        );
    }

    for _ in 0..iters {
        let start = Instant::now();
        let mut painter = VelloScenePainter::new(&mut scene);
        painter.reset();
        paint_scene(
            &mut painter,
            document.as_mut(),
            scale,
            render_width,
            render_height,
            0,
            0,
        );
        times_us.push(start.elapsed().as_micros());
    }

    times_us.sort_unstable();
    let min = times_us[0];
    let max = times_us[times_us.len() - 1];
    let median = times_us[times_us.len() / 2];
    let mean: u128 = times_us.iter().sum::<u128>() / times_us.len() as u128;
    println!("paint_scene: min {min}us / median {median}us / mean {mean}us / max {max}us");
}
