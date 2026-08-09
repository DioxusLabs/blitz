use anyrender::{ImageRenderer as _, PaintScene as _};
use blitz_dom::util::Color;
use blitz_paint::paint_scene;
use image::{ImageBuffer, ImageFormat};
use peniko::Fill;
use peniko::kurbo::Rect;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use url::Url;

use super::parse_and_resolve_document;
use crate::{BufferKind, HEIGHT, SCALE, SubtestCounts, TestFlags, ThreadCtx, WIDTH};

pub fn process_ref_test(
    ctx: &mut ThreadCtx,
    test_relative_path: &str,
    test_html: &str,
    match_references: &[String],
    mismatch_references: &[String],
    flags: &mut TestFlags,
) -> SubtestCounts {
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

    let image_is_blank = ctx.buffers.test_buffer.iter().all(|x| *x == 0);
    if image_is_blank {
        return SubtestCounts::ZERO_OF_ONE;
    }

    // A test passes if its rendering matches ANY of its `rel=match` references
    // and differs from ALL of its `rel=mismatch` references.
    let mut ref_index = 0;

    let mut matches_pass = match_references.is_empty();
    for ref_file in match_references {
        ref_index += 1;
        if render_reference_and_compare(ctx, test_relative_path, ref_file, ref_index, flags) {
            matches_pass = true;
            break;
        }
    }

    let mut mismatches_pass = true;
    for ref_file in mismatch_references {
        ref_index += 1;
        if render_reference_and_compare(ctx, test_relative_path, ref_file, ref_index, flags) {
            mismatches_pass = false;
            break;
        }
    }

    if matches_pass && mismatches_pass {
        SubtestCounts::ONE_OF_ONE
    } else {
        SubtestCounts::ZERO_OF_ONE
    }
}

/// Renders `ref_file` to the reference buffer and compares it against the already-rendered
/// test buffer. Returns `true` if the two renderings are considered equal.
fn render_reference_and_compare(
    ctx: &mut ThreadCtx,
    test_relative_path: &str,
    ref_file: &str,
    ref_index: usize,
    flags: &mut TestFlags,
) -> bool {
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
    if ctx.grid_lanes_re.is_match(&ref_html) {
        *flags |= TestFlags::USES_GRID_LANES;
    }

    let suffix = if ref_index == 1 {
        String::new()
    } else {
        format!("-{ref_index}")
    };
    let ref_out_path = ctx
        .out_dir
        .join(format!("{test_relative_path}-ref{suffix}.png"));
    render_html_to_buffer(
        ctx,
        BufferKind::Ref,
        &ref_relative_path,
        &ref_out_path,
        &ref_html,
    );

    if ctx.buffers.test_buffer == ctx.buffers.ref_buffer {
        return true;
    }

    let test_image = ImageBuffer::from_raw(WIDTH, HEIGHT, ctx.buffers.test_buffer.clone()).unwrap();
    let ref_image = ImageBuffer::from_raw(WIDTH, HEIGHT, ctx.buffers.ref_buffer.clone()).unwrap();

    let diff = dify::diff::get_results(test_image, ref_image, 0.1f32, true, None, &None, &None);

    if let Some(diff) = diff {
        let path = ctx
            .out_dir
            .join(format!("{test_relative_path}-diff{suffix}.png"));
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        diff.1.save_with_format(path, ImageFormat::Png).unwrap();
        false
    } else {
        true
    }
}

fn render_html_to_buffer(
    ctx: &mut ThreadCtx,
    buffer_kind: BufferKind,
    relative_path: &str,
    out_path: &Path,
    html: &str,
) {
    let mut document = parse_and_resolve_document(ctx, html, relative_path);

    // Determine height to render
    // let computed_height = document.as_ref().root_element().final_layout.size.height;
    // let render_height = (computed_height as u32).clamp(HEIGHT, 4000);
    let render_height = HEIGHT;

    // Render document to RGBA buffer
    let buf = ctx.buffers.get_mut(buffer_kind);
    ctx.renderer.render_to_vec(
        |scene| {
            scene.reset();

            // Render white background
            scene.fill(
                Fill::NonZero,
                Default::default(),
                Color::WHITE,
                Default::default(),
                &Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
            );

            paint_scene(scene, &mut document, SCALE, WIDTH, HEIGHT, 0, 0);
        },
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
