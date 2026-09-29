//! Benchmark painting a page with anyrender_skia on Metal, offscreen.
//!
//! Uses Ganesh by default, or Graphite with `--no-default-features --features graphite`.
//!
//! Usage: skia_paint_bench <url> [width] [height] [scale] [iters]

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("skia_paint_bench only supports macOS (Metal)");
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    bench::main().await
}

#[cfg(target_os = "macos")]
mod bench {
    use anyrender::PaintScene as _;
    use anyrender_skia::{SkiaSceneCache, SkiaScenePainter};
    use blitz_dom::DocumentConfig;
    use blitz_html::HtmlDocument;
    use blitz_net::Provider;
    use blitz_paint::paint_scene;
    use blitz_traits::shell::{ColorScheme, Viewport};
    use objc2::rc::Retained;
    use objc2_metal::{MTLCreateSystemDefaultDevice, MTLDevice as _};
    use reqwest::Url;
    use skia_safe::ImageInfo;
    use std::ffi::c_void;
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

        let mut encode_us = Vec::with_capacity(iters);
        let mut raster_us = Vec::with_capacity(iters);
        let mut total_us = Vec::with_capacity(iters);
        for _ in 0..iters {
            let (encode, raster) = frame();
            encode_us.push(encode);
            raster_us.push(raster);
            total_us.push(encode + raster);
        }

        print_stats("paint_scene", &mut encode_us);
        print_stats("rasterize  ", &mut raster_us);
        print_stats("total      ", &mut total_us);
    }

    pub async fn main() {
        let mut args = std::env::args().skip(1);
        let url_string = args.next().unwrap_or_else(|| "https://servo.org".into());
        let width: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1366);
        let height: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(768);
        let scale: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(2.0);
        let iters: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(100);

        let url = Url::parse(&url_string)
            .unwrap_or_else(|_| Url::parse(&format!("https://{url_string}")).expect("Invalid url"));
        let url_string = url.to_string();

        let html = match url.scheme() {
            "file" => String::from_utf8(std::fs::read(url.path()).unwrap()).unwrap(),
            _ => reqwest::Client::new()
                .get(url)
                .header("User-Agent", USER_AGENT)
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
        };

        let net = Arc::new(Provider::new(None));
        let render_width = (width as f64 * scale) as u32;
        let render_height = (height as f64 * scale) as u32;
        let mut document = HtmlDocument::from_html(
            &html,
            DocumentConfig {
                base_url: Some(url_string.clone()),
                net_provider: Some(Arc::clone(&net) as _),
                viewport: Some(Viewport::new(
                    render_width,
                    render_height,
                    scale as f32,
                    ColorScheme::Light,
                )),
                ..Default::default()
            },
        );
        loop {
            document.resolve(0.0);
            if net.is_empty() {
                break;
            }
        }
        document.as_mut().resolve(0.0);

        let engine = if cfg!(feature = "graphite") {
            "graphite"
        } else {
            "ganesh"
        };
        println!(
            "Loaded {url_string} at {width}x{height}@{scale}x; running {iters} paint iterations (skia {engine})"
        );

        let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
        let queue = device
            .newCommandQueue()
            .expect("unable to create command queue");
        let device_ptr = Retained::as_ptr(&device) as *mut c_void;
        let queue_ptr = Retained::as_ptr(&queue) as *mut c_void;
        let image_info =
            ImageInfo::new_n32_premul((render_width as i32, render_height as i32), None);
        let mut cache = SkiaSceneCache::default();

        #[cfg(feature = "graphite")]
        {
            use skia_safe::gpu::{
                Mipmapped,
                graphite::{self, InsertRecordingInfo, mtl},
            };

            let backend = unsafe { mtl::BackendContext::new(device_ptr, queue_ptr) };
            let mut context =
                mtl::context_factory::make_metal(&backend, None).expect("no Graphite context");
            let mut recorder = context.make_recorder(None).expect("no Graphite recorder");
            let mut surface = graphite::surfaces::render_target(
                &mut recorder,
                &image_info,
                Mipmapped::No,
                None,
                None,
            )
            .expect("unable to create surface");

            bench(iters, || {
                let encode_start = Instant::now();
                {
                    let mut painter =
                        SkiaScenePainter::new_graphite(surface.canvas(), &mut cache, &mut recorder);
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
                cache.next_gen();
                let encode = encode_start.elapsed().as_micros();

                let raster_start = Instant::now();
                let mut recording = recorder.snap().expect("unable to snap recording");
                context.insert_recording(&InsertRecordingInfo::new(&mut recording));
                context.submit_and_wait();
                (encode, raster_start.elapsed().as_micros())
            });
        }

        #[cfg(not(feature = "graphite"))]
        {
            use skia_safe::gpu::{self, Budgeted, SurfaceOrigin, mtl};

            let backend = unsafe {
                mtl::BackendContext::new(device_ptr as mtl::Handle, queue_ptr as mtl::Handle)
            };
            let mut context =
                gpu::direct_contexts::make_metal(&backend, None).expect("no Ganesh context");
            let mut surface = gpu::surfaces::render_target(
                &mut context,
                Budgeted::Yes,
                &image_info,
                None,
                SurfaceOrigin::TopLeft,
                None,
                false,
                None,
            )
            .expect("unable to create surface");

            bench(iters, || {
                let encode_start = Instant::now();
                {
                    let mut painter = SkiaScenePainter::new(surface.canvas(), &mut cache);
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
                cache.next_gen();
                let encode = encode_start.elapsed().as_micros();

                let raster_start = Instant::now();
                context.flush_submit_and_sync_cpu();
                (encode, raster_start.elapsed().as_micros())
            });
        }
    }
}
