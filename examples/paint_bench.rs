//! Benchmark the paint phase (blitz-paint `paint_scene` -> renderer command encoding).
//!
//! Supports the vello, vello_cpu and vello_hybrid anyrender backends. Only scene/command
//! encoding is measured — no rasterization or GPU work (except vello_hybrid image uploads,
//! which are cached after the first frame).
//!
//! Usage: paint_bench <url> [width] [height] [scale] [iters] [backend]
//!   backend: vello (default) | cpu | hybrid

use anyrender::PaintScene as _;
use anyrender_vello::VelloScenePainter;
use anyrender_vello_cpu::VelloCpuScenePainter;
use anyrender_vello_hybrid::{ImageManager, VelloHybridScenePainter};
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use reqwest::Url;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use std::time::Instant;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

const WARMUP_ITERS: usize = 5;

fn bench(iters: usize, mut paint: impl FnMut()) {
    for _ in 0..WARMUP_ITERS {
        paint();
    }

    let mut times_us: Vec<u128> = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        paint();
        times_us.push(start.elapsed().as_micros());
    }

    times_us.sort_unstable();
    let min = times_us[0];
    let max = times_us[times_us.len() - 1];
    let median = times_us[times_us.len() / 2];
    let mean: u128 = times_us.iter().sum::<u128>() / times_us.len() as u128;
    println!("paint_scene: min {min}us / median {median}us / mean {mean}us / max {max}us");
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let url_string = args.next().unwrap_or_else(|| "https://servo.org".into());
    let width: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1366);
    let height: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(768);
    let scale: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(2.0);
    let iters: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(100);
    let backend: String = args.next().unwrap_or_else(|| "vello".into());

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

    println!(
        "Loaded {url_string} at {width}x{height}@{scale}x; running {iters} paint iterations ({backend} backend)"
    );

    let render_width = (width as f64 * scale) as u32;
    let render_height = (height as f64 * scale) as u32;

    match backend.as_str() {
        "vello" => {
            let mut scene = vello::Scene::new();
            bench(iters, || {
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
            });
        }
        "cpu" | "vello_cpu" => {
            let mut painter = VelloCpuScenePainter {
                render_ctx: vello_cpu::RenderContext::new(
                    render_width as u16,
                    render_height as u16,
                ),
                resources: vello_cpu::Resources::new(),
            };
            bench(iters, || {
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
            });
        }
        "hybrid" | "vello_hybrid" => {
            let context = wgpu_context::WGPUContext::new();
            let device_handle = wgpu_context::DeviceHandle::new_from_compatible_surface(
                context.instance.clone(),
                None,
                None,
                None,
            )
            .await
            .expect("Failed to create wgpu device");

            let mut renderer = vello_hybrid::Renderer::new(
                &device_handle.device,
                &vello_hybrid::RenderTargetConfig {
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    width: render_width,
                    height: render_height,
                },
            );
            let mut resources = vello_hybrid::Resources::new();
            let mut scene = vello_hybrid::Scene::new(render_width as u16, render_height as u16);
            let mut image_cache = FxHashMap::default();
            let mut texture_bindings = FxHashMap::default();

            bench(iters, || {
                let mut encoder = device_handle
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                let image_manager = ImageManager::new(
                    &mut renderer,
                    &mut resources,
                    &device_handle.device,
                    &device_handle.queue,
                    &mut encoder,
                    &mut image_cache,
                );
                let mut painter = VelloHybridScenePainter::new(
                    &mut scene,
                    image_manager,
                    &mut texture_bindings,
                    &device_handle,
                );
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
                drop(painter);
                device_handle.queue.submit([encoder.finish()]);
            });
        }
        other => {
            eprintln!("Unknown backend {other:?}. Expected vello, cpu, or hybrid.");
            std::process::exit(1);
        }
    }
}
