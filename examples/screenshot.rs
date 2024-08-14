//! Load first CLI argument as a url. Fallback to google.com if no CLI argument is provided.

use blitz::render_to_buffer;
use blitz_dom::{HtmlDocument, Viewport};
use reqwest::Url;
use std::{fs::File, io::Write, path::Path};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

#[tokio::main]
async fn main() {
    let url = std::env::args()
        .skip(1)
        .next()
        .unwrap_or_else(|| "https://www.google.com".into());

    println!("{}", url);

    // Assert that url is valid
    let url = url.to_owned();
    Url::parse(&url).expect("Invalid url");

    // Fetch HTML from URL
    let html = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .call()
        .unwrap()
        .into_string()
        .unwrap();

    // Setup viewport. TODO: make configurable.
    let scale = 2;
    let width = 1200;
    let height = 800;

    // Create HtmlDocument
    let mut document = HtmlDocument::from_html(&html, Some(url), Vec::new());
    document
        .as_mut()
        .set_viewport(Viewport::new(width * scale, height * scale, scale as f32));

    // Compute style, layout, etc for HtmlDocument
    document.as_mut().resolve();

    // Determine height to render
    let computed_height = document.as_ref().root_element().final_layout.size.height;
    let render_height = (computed_height as u32).max(height);

    // Render document to RGBA buffer
    let buffer = render_to_buffer(
        document.as_ref(),
        Viewport::new(width * scale, render_height * scale, scale as f32),
    )
    .await;

    // Determine output path, and open a file at that path. TODO: make configurable.
    let out_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/output/screenshot")
        .with_extension("png");

    // Encode buffer as PNG and write it to a file
    let mut file = File::create(&out_path).unwrap();
    write_png(&mut file, &buffer, width * scale, render_height * scale);

    // Log result.
    println!("Wrote result ({width}x{height}) to {}", out_path.display());
}

fn write_png<W: Write>(writer: W, buffer: &[u8], width: u32, height: u32) {
    // Set pixels-per-meter. TODO: make configurable.
    const PPM: u32 = (144.0 * 39.3701) as u32;

    // Create PNG encoder
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: PPM,
        yppu: PPM,
        unit: png::Unit::Meter,
    }));

    // Write PNG data to writer
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&buffer).unwrap();
    writer.finish().unwrap();
}
