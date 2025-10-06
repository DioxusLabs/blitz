use anyrender::ImageRenderer as _;
use blitz_paint::paint_scene;
use image::{ImageBuffer, ImageFormat};
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use url::Url;

use super::parse_and_resolve_document;
use crate::{BufferKind, HEIGHT, SCALE, SubtestCounts, TestFlags, ThreadCtx, WIDTH};

#[allow(clippy::too_many_arguments)]
pub fn process_ref_test(
    ctx: &mut ThreadCtx,
    test_relative_path: &str,
    test_html: &str,
    ref_file: &str,
    flags: &mut TestFlags,
) -> SubtestCounts {
    let ref_url: Url = ctx
        .dummy_base_url
        .join(test_relative_path)
        .unwrap()
        .join(ref_file)
        .unwrap();
    let ref_relative_path = ref_url.path().strip_prefix('/').unwrap().to_string();
    let ref_path = ctx.wpt_dir.join(&ref_relative_path);
    let ref_html = fs::read_to_string(ref_path).expect("Ref file not found.");

    if ctx.float_re.is_match(&ref_html) {
        *flags |= TestFlags::USES_FLOAT;
    }
    if ctx.intrinsic_re.is_match(&ref_html) {
        *flags |= TestFlags::USES_INTRINSIC_SIZE;
    }
    if ctx.calc_re.is_match(&ref_html) {
        *flags |= TestFlags::USES_CALC;
    }
    if ctx.direction_re.is_match(&ref_html) {
        *flags |= TestFlags::USES_DIRECTION;
    }
    if ctx.writing_mode_re.is_match(&ref_html) {
        *flags |= TestFlags::USES_WRITING_MODE;
    }
    if ctx.subgrid_re.is_match(&ref_html) {
        *flags |= TestFlags::USES_SUBGRID;
    }
    if ctx.masonry_re.is_match(&ref_html) {
        *flags |= TestFlags::USES_MASONRY;
    }

    let test_out_path = ctx
        .out_dir
        .join(format!("{}{}", test_relative_path, "-test.png"));
    render_html_to_buffer(
        ctx,
        BufferKind::Test,
        test_relative_path,
        &test_out_path,
        test_html,
    );

    let ref_out_path = ctx
        .out_dir
        .join(format!("{}{}", test_relative_path, "-ref.png"));
    render_html_to_buffer(
        ctx,
        BufferKind::Ref,
        &ref_relative_path,
        &ref_out_path,
        &ref_html,
    );

    let image_is_blank = ctx.buffers.test_buffer.iter().all(|x| *x == 0);
    if image_is_blank {
        return SubtestCounts::ZERO_OF_ONE;
    }

    if ctx.buffers.test_buffer == ctx.buffers.ref_buffer {
        return SubtestCounts::ONE_OF_ONE;
    }

    let test_image = ImageBuffer::from_raw(WIDTH, HEIGHT, ctx.buffers.test_buffer.clone()).unwrap();
    let ref_image = ImageBuffer::from_raw(WIDTH, HEIGHT, ctx.buffers.ref_buffer.clone()).unwrap();

    let diff = dify::diff::get_results(test_image, ref_image, 0.1f32, true, None, &None, &None);

    if let Some(diff) = diff {
        let path = ctx
            .out_dir
            .join(format!("{}{}", test_relative_path, "-diff.png"));
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        diff.1.save_with_format(path, ImageFormat::Png).unwrap();
        SubtestCounts::ZERO_OF_ONE
    } else {
        SubtestCounts::ONE_OF_ONE
    }
}

fn render_html_to_buffer(
    ctx: &mut ThreadCtx,
    buffer_kind: BufferKind,
    relative_path: &str,
    out_path: &Path,
    html: &str,
) {
    let document = parse_and_resolve_document(ctx, html, relative_path);

    // Determine height to render
    // let computed_height = document.as_ref().root_element().final_layout.size.height;
    // let render_height = (computed_height as u32).clamp(HEIGHT, 4000);
    let render_height = HEIGHT;

    // Render document to RGBA buffer
    let buf = ctx.buffers.get_mut(buffer_kind);
    ctx.renderer.render(
        |scene| paint_scene(scene, document.as_ref(), SCALE, WIDTH, HEIGHT),
        buf,
    );

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
