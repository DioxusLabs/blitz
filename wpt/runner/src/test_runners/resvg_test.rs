//! A test from resvg's suite (see `crate::resvg`): renders an SVG in one
//! `Mode` and compares it against resvg's PNG, or for parity against its own
//! `<img>` rendering.

use blitz_traits::shell::{ColorScheme, Viewport};
use image::{ImageBuffer, ImageFormat, RgbaImage};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use super::ref_test::render_html_to_buffer;
use crate::manifest::artifact_name;
use crate::resvg::Mode;
use crate::{BufferKind, HEIGHT, SubtestCounts, ThreadCtx, WIDTH};

/// resvg's references come from tiny-skia, not vello_cpu, so anti-aliased
/// edges and gradients differ slightly almost everywhere: allow 100 pixels
/// (0.25% of a 200x200 render) to differ by more than 64/255. That is about
/// one edge's worth, so small features such as `shape-rendering="crispEdges"`
/// or a misplaced dash still fail. `RESVG_PNG_FUZZY=<threshold>;<pixels>`
/// overrides it, to measure the suite at other tolerances.
const PNG_TOLERANCE: Tolerance = Tolerance {
    threshold: 64,
    max_pixels: 100,
};
/// Native vs usvg runs through the same rasterizer.
const PARITY_TOLERANCE: Tolerance = Tolerance {
    threshold: 16,
    max_pixels: 90,
};

/// Renderings match if at most `max_pixels` pixels differ by more than
/// `threshold` in any channel. Unlike WPT's fuzzy tolerance it ignores small
/// differences entirely, which comparing against another rasterizer's output
/// needs.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tolerance {
    threshold: u8,
    max_pixels: u64,
}

impl Tolerance {
    fn parse(content: &str) -> Option<Self> {
        let (threshold, max_pixels) = content.split_once(';')?;
        Some(Self {
            threshold: threshold.trim().parse().ok()?,
            max_pixels: max_pixels.trim().parse().ok()?,
        })
    }

    fn matches(self, test_buffer: &[u8], ref_buffer: &[u8]) -> bool {
        let differing_pixels = test_buffer
            .as_chunks::<4>()
            .0
            .iter()
            .zip(ref_buffer.as_chunks::<4>().0.iter())
            .filter(|(test_px, ref_px)| {
                test_px
                    .iter()
                    .zip(ref_px.iter())
                    .any(|(t, r)| t.abs_diff(*r) > self.threshold)
            })
            .count();
        differing_pixels as u64 <= self.max_pixels
    }
}

static PNG_TOLERANCE_OVERRIDE: LazyLock<Tolerance> = LazyLock::new(|| {
    let Ok(content) = std::env::var("RESVG_PNG_FUZZY") else {
        return PNG_TOLERANCE;
    };
    Tolerance::parse(&content).unwrap_or_else(|| {
        panic!("invalid RESVG_PNG_FUZZY {content:?}: expected <threshold 0-255>;<pixels>")
    })
});

/// Renders `svg_source`, the SVG at `relative_path` (relative to the suite
/// dir), in `mode` and compares it against its reference.
pub fn process_resvg_test(
    ctx: &mut ThreadCtx,
    relative_path: &str,
    test_url: &str,
    svg_source: &str,
    mode: Mode,
) -> SubtestCounts {
    let svg_path = ctx.wpt_dir.join(relative_path);
    let png = image::open(svg_path.with_extension("png"))
        .unwrap()
        .to_rgba8();
    let (width, height) = png.dimensions();
    assert!(
        width <= WIDTH && height <= HEIGHT,
        "{width}x{height} reference is too large"
    );
    // resvg's harness renders usvg's size scaled to the PNG's width
    let tree = usvg::Tree::from_str(svg_source, &usvg::Options::default()).unwrap();
    let scale = width as f32 / tree.size().width();

    // The test's URL path, which the net provider decodes back to a file path
    let url_path = relative_path.replace('%', "%25").replace('#', "%23");
    let file_name = url_path.rsplit('/').next().unwrap();
    let img_html = format!(
        "<!DOCTYPE html>\n<style>body {{ margin: 0 }} img {{ display: block; width: {width}px; height: {height}px }}</style>\n<img src=\"{}\">\n",
        html_escape::encode_double_quoted_attribute(file_name)
    );

    let artifact = artifact_name(test_url);
    let out_path = |suffix: &str| ctx.out_dir.join(format!("{artifact}-{suffix}.png"));
    let (test_out, ref_out, diff_out) = (out_path("test"), out_path("ref"), out_path("diff"));

    let render = |ctx: &mut ThreadCtx, kind: BufferKind, out: &Path, doc: bool| {
        let (path, html, scale) = if doc {
            (url_path.clone(), svg_source, scale)
        } else {
            (format!("{url_path}.html"), img_html.as_str(), 1.0)
        };
        let viewport = Viewport::new(width, height, scale, ColorScheme::Light);
        let default_viewport = std::mem::replace(&mut ctx.viewport, viewport);
        render_html_to_buffer(ctx, kind, &path, out, html);
        ctx.viewport = default_viewport;
    };

    let tolerance = match mode {
        Mode::Img | Mode::Doc => {
            render(ctx, BufferKind::Test, &test_out, mode == Mode::Doc);
            fill_with_png(&mut ctx.buffers.ref_buffer, &png);
            *PNG_TOLERANCE_OVERRIDE
        }
        Mode::Parity => {
            render(ctx, BufferKind::Test, &test_out, true);
            render(ctx, BufferKind::Ref, &ref_out, false);
            PARITY_TOLERANCE
        }
    };

    // Only the PNG's area is compared, so content overflowing the viewport doesn't count
    for (buffer, out) in [
        (&mut ctx.buffers.test_buffer, &test_out),
        (&mut ctx.buffers.ref_buffer, &ref_out),
    ] {
        clear_outside(buffer, width, height);
        save_png(out, buffer);
    }
    let (test_buffer, ref_buffer) = (&ctx.buffers.test_buffer, &ctx.buffers.ref_buffer);
    if test_buffer == ref_buffer {
        return SubtestCounts::ONE_OF_ONE;
    }
    let test_image = ImageBuffer::from_raw(WIDTH, HEIGHT, test_buffer.clone()).unwrap();
    let ref_image = ImageBuffer::from_raw(WIDTH, HEIGHT, ref_buffer.clone()).unwrap();
    if let Some(diff) =
        dify::diff::get_results(test_image, ref_image, 0.1, true, None, &None, &None)
    {
        diff.1.save_with_format(diff_out, ImageFormat::Png).unwrap();
    }
    if tolerance.matches(test_buffer, ref_buffer) {
        SubtestCounts::ONE_OF_ONE
    } else {
        SubtestCounts::ZERO_OF_ONE
    }
}

/// Fills an RGBA buffer the size of the runner's with `png` at its top-left
/// corner over the white page the runner renders on.
fn fill_with_png(buffer: &mut Vec<u8>, png: &RgbaImage) {
    buffer.clear();
    buffer.resize((WIDTH * HEIGHT * 4) as usize, 255);
    for (x, y, pixel) in png.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let offset = ((y * WIDTH + x) * 4) as usize;
        for (channel, value) in [r, g, b].into_iter().enumerate() {
            let blended = (value as u32 * a as u32 + 255 * (255 - a as u32) + 127) / 255;
            buffer[offset + channel] = blended as u8;
        }
    }
}

/// Sets the pixels of `buffer` outside the top-left `width`x`height` to white.
fn clear_outside(buffer: &mut [u8], width: u32, height: u32) {
    let rows = buffer.as_chunks_mut::<{ (WIDTH * 4) as usize }>().0;
    for (y, row) in rows.iter_mut().enumerate() {
        let start = if (y as u32) < height { width } else { 0 };
        row[(start * 4) as usize..].fill(255);
    }
}

fn save_png(path: &Path, buffer: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let image: RgbaImage = ImageBuffer::from_raw(WIDTH, HEIGHT, buffer.to_vec()).unwrap();
    image.save_with_format(path, ImageFormat::Png).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerance() {
        let tolerance = Tolerance::parse("64; 1").unwrap();
        assert_eq!(
            tolerance,
            Tolerance {
                threshold: 64,
                max_pixels: 1
            }
        );
        let a = [0u8, 0, 0, 255, 0, 0, 0, 255];
        assert!(tolerance.matches(&a, &[64, 0, 0, 255, 200, 0, 0, 255]));
        assert!(!tolerance.matches(&a, &[65, 0, 0, 255, 200, 0, 0, 255]));
        assert_eq!(Tolerance::parse("256;1"), None);
        assert_eq!(Tolerance::parse("64"), None);
    }
}
