use blitz_dom::net::Resource;
use blitz_dom::HtmlDocument;
use blitz_renderer_vello::VelloImageRenderer;
use blitz_traits::net::SharedProvider;
use log::error;
use parley::FontContext;
use reqwest::Url;

use tokio::time::timeout;

use image::{ImageBuffer, ImageFormat};
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::{clone_font_ctx, BlitzContext, TestResult, HEIGHT, WIDTH};

#[allow(clippy::too_many_arguments)]
pub async fn process_ref_test(
    renderer: &mut VelloImageRenderer,
    font_ctx: &FontContext,
    test_buffer: &mut Vec<u8>,
    ref_buffer: &mut Vec<u8>,
    test_url: &Url,
    test_file_contents: &str,
    ref_file: &str,
    base_url: &Url,
    blitz_context: &mut BlitzContext,
    out_dir: &Path,
) -> TestResult {
    let ref_url: Url = test_url.join(".").unwrap().join(ref_file).unwrap();

    let reference_file_contents = reqwest::get(ref_url.clone())
        .await
        .expect("Could not read file.")
        .text()
        .await;

    if reference_file_contents.is_err() {
        error!(
            "Skipping test file: {}. Reference file missing {}",
            test_url, ref_url
        );
        panic!("Skipping test file: {}. No reference found.", ref_url);
    }

    let reference_file_contents = reference_file_contents.unwrap();

    let test_base_url = test_url.to_string().replace(base_url.as_str(), "");
    let test_out_path = out_dir.join(format!("{}{}", test_base_url, "-test.png"));
    render_html_to_buffer(
        blitz_context,
        renderer,
        font_ctx,
        test_file_contents,
        test_url,
        test_buffer,
        &test_out_path,
    )
    .await;

    let ref_out_path = out_dir.join(format!("{}{}", test_base_url, "-ref.png"));
    render_html_to_buffer(
        blitz_context,
        renderer,
        font_ctx,
        &reference_file_contents,
        &ref_url,
        ref_buffer,
        &ref_out_path,
    )
    .await;

    if test_buffer == ref_buffer {
        return TestResult::Pass;
    }

    let test_image = ImageBuffer::from_raw(WIDTH, HEIGHT, test_buffer.clone()).unwrap();
    let ref_image = ImageBuffer::from_raw(WIDTH, HEIGHT, ref_buffer.clone()).unwrap();

    let diff = dify::diff::get_results(test_image, ref_image, 0.1f32, true, None, &None, &None);

    if let Some(diff) = diff {
        let path = out_dir.join(format!("{}{}", test_base_url, "-diff.png"));
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        diff.1.save_with_format(path, ImageFormat::Png).unwrap();
        TestResult::Fail
    } else {
        TestResult::Pass
    }
}

async fn render_html_to_buffer(
    blitz_context: &mut BlitzContext,
    renderer: &mut VelloImageRenderer,
    font_ctx: &FontContext,
    html: &str,
    base_url: &Url,
    buf: &mut Vec<u8>,
    out_path: &Path,
) {
    let mut document = HtmlDocument::from_html(
        html,
        Some(base_url.to_string()),
        Vec::new(),
        Arc::clone(&blitz_context.net) as SharedProvider<Resource>,
        Some(clone_font_ctx(font_ctx)),
    );

    document
        .as_mut()
        .set_viewport(blitz_context.viewport.clone());

    while !blitz_context.net.is_empty() {
        let Ok(Some(res)) =
            timeout(Duration::from_millis(500), blitz_context.receiver.recv()).await
        else {
            break;
        };
        document.as_mut().load_resource(res);
    }

    // Compute style, layout, etc for HtmlDocument
    document.as_mut().resolve();

    // Determine height to render
    // let computed_height = document.as_ref().root_element().final_layout.size.height;
    // let render_height = (computed_height as u32).clamp(HEIGHT, 4000);
    let render_height = HEIGHT;

    // Render document to RGBA buffer
    renderer.render_document(document.as_ref(), buf);

    fs::create_dir_all(out_path.parent().unwrap()).unwrap();
    let mut file = File::create(out_path).unwrap();
    write_png(&mut file, buf, WIDTH, render_height);
}

// Copied from screenshot.rs
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
    writer.write_image_data(buffer).unwrap();
    writer.finish().unwrap();
}
