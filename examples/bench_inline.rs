//! Benchmark layout of a page: initial layout + relayouts at alternating viewport widths.
//! Usage: cargo run --release --example bench_inline <url-or-file> [iterations]

use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_traits::shell::{ColorScheme, Viewport};
use reqwest::Url;
use std::sync::Arc;
use std::time::Instant;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

#[tokio::main]
async fn main() {
    let url_string = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://en.wikipedia.org/wiki/Roman_Egypt".into());
    let iterations: usize = std::env::args()
        .nth(2)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(20);

    let url = Url::parse(&url_string)
        .unwrap_or_else(|_| Url::parse(&format!("https://{url_string}")).expect("Invalid url"));
    let url_string = url.to_string();

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

    let scale = 2.0f32;
    let height = 800u32;
    let width = 1200u32;

    let net = Arc::new(Provider::new(None));

    let base_url = std::env::var("BENCH_BASE_URL").ok().or(Some(url_string));

    let mut document = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            base_url,
            net_provider: Some(Arc::clone(&net) as _),
            viewport: Some(Viewport::new(
                width * (scale as u32),
                height * (scale as u32),
                scale,
                ColorScheme::Light,
            )),
            ..Default::default()
        },
    );

    // Load assets
    loop {
        document.resolve(0.0);
        if net.is_empty() {
            break;
        }
    }

    // Initial full layouts at alternating widths
    let mut initial_times = Vec::new();
    let mut relayout_times = Vec::new();

    let start = Instant::now();
    document.as_mut().resolve(0.0);
    initial_times.push(start.elapsed().as_secs_f64() * 1000.0);

    for i in 0..iterations {
        let w = if i % 2 == 0 { 1210 } else { 1200 };
        document.as_mut().set_viewport(Viewport::new(
            w * (scale as u32),
            height * (scale as u32),
            scale,
            ColorScheme::Light,
        ));
        let start = Instant::now();
        document.as_mut().resolve(0.0);
        relayout_times.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    relayout_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = relayout_times.first().copied().unwrap_or(0.0);
    let median = relayout_times[relayout_times.len() / 2];
    let mean: f64 = relayout_times.iter().sum::<f64>() / relayout_times.len() as f64;

    println!("initial resolve: {:.2}ms", initial_times[0]);
    println!("relayout (n={iterations}): min {min:.2}ms / median {median:.2}ms / mean {mean:.2}ms");
}
