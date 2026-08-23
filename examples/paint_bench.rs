//! Benchmark the paint phase (blitz-paint `paint_scene` -> renderer command encoding)
//! and the rasterization phase (renderer-side work: GPU dispatch / CPU rendering).
//!
//! Supports the vello, vello_cpu and vello_hybrid anyrender backends.
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

fn print_stats(label: &str, times_us: &mut [u128]) {
    times_us.sort_unstable();
    let min = times_us[0];
    let max = times_us[times_us.len() - 1];
    let median = times_us[times_us.len() / 2];
    let mean: u128 = times_us.iter().sum::<u128>() / times_us.len() as u128;
    println!("{label}: min {min}us / median {median}us / mean {mean}us / max {max}us");
}

/// Run `iters` iterations of a two-phase (encode, rasterize) frame and report
/// per-phase timing statistics.
fn bench(iters: usize, mut frame: impl FnMut() -> (u128, u128)) {
    for _ in 0..WARMUP_ITERS {
        frame();
    }

    let mut encode_us: Vec<u128> = Vec::with_capacity(iters);
    let mut raster_us: Vec<u128> = Vec::with_capacity(iters);
    for _ in 0..iters {
        let (encode, raster) = frame();
        encode_us.push(encode);
        raster_us.push(raster);
    }

    print_stats("paint_scene", &mut encode_us);
    print_stats("rasterize  ", &mut raster_us);
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
            let mut context = wgpu_context::WGPUContext::new();
            let buffer_renderer = context
                .create_buffer_renderer(wgpu_context::BufferRendererConfig {
                    width: render_width,
                    height: render_height,
                    usage: wgpu::TextureUsages::STORAGE_BINDING,
                })
                .await
                .expect("No compatible device found");
            let mut renderer = vello::Renderer::new(
                buffer_renderer.device(),
                vello::RendererOptions {
                    use_cpu: false,
                    num_init_threads: std::num::NonZeroUsize::new(1),
                    antialiasing_support: vello::AaSupport::area_only(),
                    pipeline_cache: None,
                },
            )
            .expect("Failed to create vello renderer");

            let mut scene = vello::Scene::new();
            bench(iters, || {
                let encode_start = Instant::now();
                {
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
                let encode = encode_start.elapsed().as_micros();

                let raster_start = Instant::now();
                renderer
                    .render_to_texture(
                        buffer_renderer.device(),
                        buffer_renderer.queue(),
                        &scene,
                        &buffer_renderer.target_texture_view(),
                        &vello::RenderParams {
                            base_color: vello::peniko::Color::TRANSPARENT,
                            width: render_width,
                            height: render_height,
                            antialiasing_method: vello::AaConfig::Area,
                        },
                    )
                    .expect("Failed to render to texture");
                buffer_renderer
                    .device()
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
                (encode, raster_start.elapsed().as_micros())
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
            let mut buffer = vec![0u8; render_width as usize * render_height as usize * 4];
            bench(iters, || {
                let encode_start = Instant::now();
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
                let encode = encode_start.elapsed().as_micros();

                let raster_start = Instant::now();
                painter.render_ctx.flush();
                painter.render_ctx.render(
                    vello_cpu::PixmapMut::new(
                        render_width as u16,
                        render_height as u16,
                        &mut buffer,
                    )
                    .unwrap(),
                    &mut painter.resources,
                );
                (encode, raster_start.elapsed().as_micros())
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

            let target_texture = device_handle
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("paint_bench target"),
                    size: wgpu::Extent3d {
                        width: render_width,
                        height: render_height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
            let target_texture_view =
                target_texture.create_view(&wgpu::TextureViewDescriptor::default());

            bench(iters, || {
                let encode_start = Instant::now();
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
                let encode = encode_start.elapsed().as_micros();

                let raster_start = Instant::now();
                let mut hybrid_texture_bindings = vello_hybrid::TextureBindings::new();
                for (resource_id, texture_view) in texture_bindings.iter() {
                    hybrid_texture_bindings.insert(
                        vello_common::TextureId(resource_id.into_ffi()),
                        texture_view.clone(),
                    );
                }
                renderer
                    .render(
                        &scene,
                        &mut resources,
                        &device_handle.device,
                        &device_handle.queue,
                        &mut encoder,
                        &vello_hybrid::RenderSize {
                            width: render_width,
                            height: render_height,
                        },
                        &target_texture_view,
                        &hybrid_texture_bindings,
                    )
                    .expect("Failed to render to texture");
                device_handle.queue.submit([encoder.finish()]);
                device_handle
                    .device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
                scene.reset();
                (encode, raster_start.elapsed().as_micros())
            });
        }
        other => {
            eprintln!("Unknown backend {other:?}. Expected vello, cpu, or hybrid.");
            std::process::exit(1);
        }
    }
}
