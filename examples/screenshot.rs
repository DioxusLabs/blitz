//! Load first CLI argument as a url. Fallback to google.com if no CLI argument is provided.

use std::{fs::File, path::Path};

use blitz::render_to_buffer;
use blitz_dom::{HtmlDocument, Viewport};
use reqwest::Url;

#[tokio::main]
async fn main() {
    let url = std::env::args()
        .skip(1)
        .next()
        .unwrap_or_else(|| "https://www.google.com".into());

    const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";
    println!("{}", url);

    // Assert that url is valid
    let url = url.to_owned();
    Url::parse(&url).expect("Invalid url");

    let html = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .call()
        .unwrap()
        .into_string()
        .unwrap();

    let scale = 2;
    let width = 800 * scale;
    let height = 600 * scale;

    let viewport = Viewport::new(width, height, scale as f32);
    let mut document = HtmlDocument::from_html(&html, Some(url), Vec::new());
    document.as_mut().set_viewport(viewport.clone());
    document.as_mut().resolve();

    let buffer = render_to_buffer(document.as_ref(), viewport).await;

    let out_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/output/screenshot")
        .with_extension("png");

    write_png(out_path.as_path(), &buffer, width, height);

    println!("Wrote result ({width}x{height}) to {out_path:?}");
}

fn write_png(out_path: &Path, buffer: &[u8], width: u32, height: u32) {
    let mut file = File::create(&out_path).unwrap();

    let mut encoder = png::Encoder::new(&mut file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    const PPM: u32 = (144.0 * 39.3701) as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: PPM,
        yppu: PPM,
        unit: png::Unit::Meter,
    }));
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&buffer).unwrap();
    writer.finish().unwrap();
}
