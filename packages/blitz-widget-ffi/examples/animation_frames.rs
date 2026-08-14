//! Smoke test for deterministic CSS animation sampling: render the animated
//! demo template at several instants of its 4s cycle and write one PNG per
//! frame. Usage: cargo run -p blitz-widget-ffi --example animation_frames -- outdir

use blitz_widget_ffi::{demo, render_html};

fn main() {
    let outdir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "anim_frames".into());
    std::fs::create_dir_all(&outdir).unwrap();

    let (width, height, scale) = (360u32, 170u32, 3.0f64);
    let html = demo::animated_html(5, false);

    let mut prev: Option<Vec<u8>> = None;
    for i in 0..=8 {
        let t = i as f64 * demo::ANIMATION_DURATION / 8.0;
        let start = std::time::Instant::now();
        let (buffer, regions) = render_html(&html, width, height, scale, t);
        let elapsed = start.elapsed();
        let differs = prev.as_ref().map(|p| p != &buffer);
        let tracked: Vec<String> = regions
            .iter()
            .filter(|r| r.action.starts_with("track:"))
            .map(|r| {
                format!(
                    "{}=({:.1},{:.1} {:.1}x{:.1})",
                    r.action, r.x, r.y, r.width, r.height
                )
            })
            .collect();
        println!(
            "t={t:.2}s  render={:.1}ms  differs_from_prev={differs:?}  {}",
            elapsed.as_secs_f64() * 1000.0,
            tracked.join("  ")
        );
        prev = Some(buffer.clone());

        let pw = (width as f64 * scale) as u32;
        let ph = (height as f64 * scale) as u32;
        let file = std::fs::File::create(format!("{outdir}/frame_{i}.png")).unwrap();
        let mut encoder = png::Encoder::new(file, pw, ph);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&buffer).unwrap();
        writer.finish().unwrap();
    }
    println!("Wrote frames to {outdir}/");
}
