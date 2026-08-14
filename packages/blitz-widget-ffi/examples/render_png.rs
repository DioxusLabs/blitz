//! Smoke test: render the demo widget HTML to a PNG and print hit regions.
//! Usage: cargo run -p blitz-widget-ffi --example render_png -- out.png [width] [height] [scale]

use blitz_widget_ffi::{demo, render_html};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "widget.png".into());
    let width: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(360);
    let height: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(170);
    let scale: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(3.0);

    let html = demo::widget_html(3, 6);
    let (buffer, regions) = render_html(&html, width, height, scale, 0.0);

    for r in &regions {
        println!(
            "{:>10}  x={:7.2} y={:7.2} w={:6.2} h={:6.2}",
            r.action, r.x, r.y, r.width, r.height
        );
    }

    let pw = (width as f64 * scale) as u32;
    let ph = (height as f64 * scale) as u32;
    let file = std::fs::File::create(&out).unwrap();
    let mut encoder = png::Encoder::new(file, pw, ph);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&buffer).unwrap();
    writer.finish().unwrap();
    println!("Wrote {out} ({pw}x{ph})");
}
