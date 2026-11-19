//! Benchmark the paint phase (blitz-paint `paint_scene` -> renderer command encoding)
//! and the rasterization phase (renderer-side work: GPU dispatch / CPU rendering).
//!
//! Supports the vello, vello_cpu and vello_gpu anyrender backends.
//!
//! Usage: paint_bench <url> [width] [height] [scale] [iters] [backend] [glyph_cache] [scroll]
//!   backend: vello (default) | cpu | gpu
//!   glyph_cache: off (default) | on
//!   scroll: <px> (static y offset, default 0) | scroll:<step_px> (advance y by step each frame)
//!
//! Environment variables:
//!   BENCH_FRESH_ATLAS=1   Reset glyph atlas / renderer resources between frames (GPU only)
//!   BENCH_PROFILE_SECS=N  Loop frames for N seconds after warmup (for external sampling profilers)

use anyrender::ImageRenderer as _;
use anyrender::PaintScene as _;
use anyrender_vello::VelloScenePainter;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use anyrender_vello_gpu::{
    ExternalTextures, GlyphAtlas, ImageCache, ImageManager, VelloGpuScenePainter,
};
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use reqwest::Url;
use std::sync::Arc;
use std::time::Instant;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

const WARMUP_ITERS: usize = 5;

fn print_stats(label: &str, times_us: &mut [u128]) {
    times_us.sort_unstable();
    let min = times_us[0];
    let max = times_us[times_us.len() - 1];
    let median = times_us[times_us.len() / 2];
    let p95 = times_us[(times_us.len() * 95 / 100).min(times_us.len() - 1)];
    let mean: u128 = times_us.iter().sum::<u128>() / times_us.len() as u128;
    println!(
        "{label}: min {min}us / median {median}us / mean {mean}us / p95 {p95}us / max {max}us"
    );
}

/// Run `iters` iterations of a two-phase (encode, rasterize) frame and report
/// cold frame + warm per-phase timing statistics.
fn bench(iters: usize, mut frame: impl FnMut(usize) -> (u128, u128)) {
    let (cold_enc, cold_ras) = frame(0);
    println!(
        "cold_frame : encode {cold_enc}us / rasterize {cold_ras}us / total {}us",
        cold_enc + cold_ras
    );

    for i in 1..WARMUP_ITERS {
        frame(i);
    }

    if let Ok(secs_str) = std::env::var("BENCH_PROFILE_SECS") {
        if let Ok(secs) = secs_str.parse::<u64>() {
            let start = Instant::now();
            let mut count = 0usize;
            while start.elapsed().as_secs() < secs {
                frame(WARMUP_ITERS + count);
                count += 1;
            }
            println!(
                "profile_loop: {count} iters in {:.2}s",
                start.elapsed().as_secs_f64()
            );
            return;
        }
    }

    let mut encode_us: Vec<u128> = Vec::with_capacity(iters);
    let mut raster_us: Vec<u128> = Vec::with_capacity(iters);
    let mut total_us: Vec<u128> = Vec::with_capacity(iters);
    for i in 0..iters {
        let (encode, raster) = frame(WARMUP_ITERS + i);
        encode_us.push(encode);
        raster_us.push(raster);
        total_us.push(encode + raster);
    }

    print_stats("paint_scene", &mut encode_us);
    print_stats("rasterize  ", &mut raster_us);
    print_stats("total      ", &mut total_us);
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
    let glyph_cache_arg: String = args.next().unwrap_or_else(|| "off".into());
    let glyph_caching = matches!(glyph_cache_arg.as_str(), "on" | "true" | "1" | "cached");
    let scroll_arg: String = args.next().unwrap_or_else(|| "0".into());
    let (scroll_base, scroll_step): (f64, f64) =
        if let Some(step) = scroll_arg.strip_prefix("scroll:") {
            (0.0, step.parse().unwrap_or(40.0))
        } else {
            (scroll_arg.parse().unwrap_or(0.0), 0.0)
        };

    let url = Url::parse(&url_string)
        .unwrap_or_else(|_| Url::parse(&format!("https://{url_string}")).expect("Invalid url"));
    let url_string = url.to_string();

    // Fetch HTML
    let html = match url.scheme() {
        "file" => {
            let path = url.to_file_path().expect("Invalid file URL");
            String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned()
        }
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
        "Loaded {url_string} at {width}x{height}@{scale}x; running {iters} paint iterations ({backend} backend, glyph_cache={glyph_caching}, scroll={scroll_arg})"
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
            bench(iters, |i| {
                let y = scroll_base + scroll_step * (i as f64);
                document
                    .as_mut()
                    .set_viewport_scroll(blitz_dom::Point { x: 0.0, y });
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
            let mut renderer = VelloCpuImageRenderer::new(render_width, render_height);
            renderer.set_glyph_caching(glyph_caching);
            let mut buffer = vec![0u8; render_width as usize * render_height as usize * 4];
            bench(iters, |i| {
                let y = scroll_base + scroll_step * (i as f64);
                document
                    .as_mut()
                    .set_viewport_scroll(blitz_dom::Point { x: 0.0, y });
                let total_start = Instant::now();
                let mut encode = 0u128;
                renderer.render(
                    |painter| {
                        let encode_start = Instant::now();
                        painter.reset();
                        paint_scene(
                            painter,
                            document.as_mut(),
                            scale,
                            render_width,
                            render_height,
                            0,
                            0,
                        );
                        encode = encode_start.elapsed().as_micros();
                    },
                    &mut buffer,
                );
                let total = total_start.elapsed().as_micros();
                (encode, total - encode)
            });
        }
        "gpu" | "vello_gpu" => {
            let context = wgpu_context::WGPUContext::new();
            let device_handle = wgpu_context::DeviceHandle::new_from_compatible_surface(
                context.instance.clone(),
                None,
                None,
                None,
            )
            .await
            .expect("Failed to create wgpu device");
            let info = device_handle.adapter.get_info();
            println!(
                "adapter    : {} ({:?}, {:?})",
                info.name, info.backend, info.device_type
            );

            let (mut renderer, mut resources) = vello_gpu::Renderer::new(
                &device_handle.device,
                &device_handle.queue,
                &vello_gpu::RenderTargetConfig {
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    width: render_width as u16,
                    height: render_height as u16,
                },
            );
            let render_size = vello_gpu::RenderSize {
                width: render_width as u16,
                height: render_height as u16,
            };
            let depth_texture_view = renderer.create_depth_texture_view(&render_size);
            let mut scene = vello_gpu::Scene::new(render_width as u16, render_height as u16);
            let mut image_cache = ImageCache::default();
            let mut texture_bindings = ExternalTextures::default();
            let mut glyphs = GlyphAtlas::new();

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

            let fresh_atlas = std::env::var("BENCH_FRESH_ATLAS")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);

            bench(iters, |i| {
                if fresh_atlas && i > 0 {
                    glyphs = GlyphAtlas::new();
                }
                let y = scroll_base + scroll_step * (i as f64);
                document
                    .as_mut()
                    .set_viewport_scroll(blitz_dom::Point { x: 0.0, y });
                let encode_start = Instant::now();
                let mut encoder = device_handle
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                let image_manager = ImageManager::new(
                    &device_handle.device,
                    &device_handle.queue,
                    &mut image_cache,
                    &mut texture_bindings,
                    &mut glyphs,
                );
                let mut painter =
                    VelloGpuScenePainter::new(&mut scene, image_manager, &device_handle)
                        .with_glyph_caching(glyph_caching);
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
                renderer
                    .render(
                        &scene,
                        &mut resources,
                        &mut encoder,
                        vello_gpu::RenderTarget {
                            view: &target_texture_view,
                            depth_view: Some(&depth_texture_view),
                        },
                        texture_bindings.bindings(),
                        vello_gpu::RenderOptions::default(),
                    )
                    .expect("Failed to render to texture");
                ImageManager::new(
                    &device_handle.device,
                    &device_handle.queue,
                    &mut image_cache,
                    &mut texture_bindings,
                    &mut glyphs,
                )
                .finish_frame();
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
            eprintln!("Unknown backend {other:?}. Expected vello, cpu, or gpu.");
            std::process::exit(1);
        }
    }
}
