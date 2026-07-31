//! CPU rendering of documents to RGBA buffers/PNGs.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::BaseDocument;
use blitz_dom::util::Color;
use blitz_paint::paint_scene;
use peniko::Fill;
use peniko::kurbo::Rect;

/// An RGBA8 image rendered from a document (or loaded from a PNG)
#[derive(Clone, PartialEq, Eq)]
pub struct Screenshot {
    pub width: u32,
    pub height: u32,
    /// RGBA8 pixel data, row-major
    pub data: Vec<u8>,
}

impl std::fmt::Debug for Screenshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Screenshot")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl Screenshot {
    /// Get the RGBA value of the pixel at `(x, y)`
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        assert!(x < self.width && y < self.height, "pixel out of bounds");
        let idx = ((y * self.width + x) * 4) as usize;
        self.data[idx..idx + 4].try_into().unwrap()
    }

    /// Encode as PNG and write to `path`, creating parent directories as needed
    pub fn save_png(&self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let file = File::create(path)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", path.display()));
        self.write_png(file);
    }

    /// Encode as PNG to an arbitrary writer
    pub fn write_png<W: Write>(&self, writer: W) {
        let mut encoder = png::Encoder::new(writer, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&self.data).unwrap();
        writer.finish().unwrap();
    }

    /// Load a PNG file as a screenshot
    pub fn load_png(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|err| format!("open {}: {err}", path.display()))?;
        let decoder = png::Decoder::new(std::io::BufReader::new(file));
        let mut reader = decoder
            .read_info()
            .map_err(|err| format!("decode {}: {err}", path.display()))?;
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|err| format!("decode {}: {err}", path.display()))?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err(format!(
                "{}: expected 8-bit RGBA png, got {:?} {:?}",
                path.display(),
                info.color_type,
                info.bit_depth
            ));
        }
        buf.truncate(info.buffer_size());
        Ok(Self {
            width: info.width,
            height: info.height,
            data: buf,
        })
    }
}

/// The result of comparing two same-sized screenshots that differ
pub struct ScreenshotDiff {
    /// Number of pixels differing by more than the tolerance in some channel
    pub differing_pixels: usize,
    /// Copy of the "actual" image with differing pixels highlighted in red
    pub diff_image: Screenshot,
}

/// Compare two screenshots pixel-by-pixel with a per-channel `tolerance` (0 for exact).
///
/// Returns `None` if the images match, and `Some(ScreenshotDiff)` (or an `Err` on
/// dimension mismatch) otherwise.
pub fn compare_screenshots(
    actual: &Screenshot,
    expected: &Screenshot,
    tolerance: u8,
) -> Result<Option<ScreenshotDiff>, String> {
    if actual.width != expected.width || actual.height != expected.height {
        return Err(format!(
            "size mismatch: actual {}x{}, expected {}x{}",
            actual.width, actual.height, expected.width, expected.height
        ));
    }

    let mut differing_pixels = 0;
    let mut diff_image = actual.clone();
    for (i, (a, e)) in actual
        .data
        .chunks_exact(4)
        .zip(expected.data.chunks_exact(4))
        .enumerate()
    {
        let differs = a
            .iter()
            .zip(e.iter())
            .any(|(a, e)| a.abs_diff(*e) > tolerance);
        if differs {
            differing_pixels += 1;
            diff_image.data[i * 4..i * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
    }

    if differing_pixels == 0 {
        return Ok(None);
    }
    Ok(Some(ScreenshotDiff {
        differing_pixels,
        diff_image,
    }))
}

/// Render `doc` to an RGBA [`Screenshot`] at the viewport's physical size, on a white
/// background, using the CPU renderer.
///
/// Styles and layout must already be resolved.
pub fn screenshot_document(doc: &mut BaseDocument) -> Screenshot {
    let (width, height) = doc.get_viewport().window_size;
    screenshot_document_with_size(doc, width, height)
}

/// Render `doc` to an RGBA [`Screenshot`] at the given physical size, on a white
/// background, using the CPU renderer.
///
/// Styles and layout must already be resolved.
pub fn screenshot_document_with_size(
    doc: &mut BaseDocument,
    width: u32,
    height: u32,
) -> Screenshot {
    let scale = doc.get_viewport().scale_f64();
    let data = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(
                Fill::NonZero,
                Default::default(),
                Color::WHITE,
                Default::default(),
                &Rect::new(0.0, 0.0, width as f64, height as f64),
            );
            paint_scene(scene, doc, scale, width, height, 0, 0);
        },
        width,
        height,
    );
    Screenshot {
        width,
        height,
        data,
    }
}
