//! CPU rendering of documents to RGBA buffers/PNGs, plus screenshot
//! comparison and snapshot assertions with on-disk failure artifacts.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::Document;
use blitz_dom::util::Color;
use blitz_paint::paint_scene;
use peniko::Fill;
use peniko::kurbo::Rect;

use crate::Harness;

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

/// Directory failure artifacts (actual/diff PNGs and DOM dumps) are written to.
///
/// Defaults to `target/blitz-test-artifacts` relative to the current directory, and can
/// be overridden with the `BLITZ_TEST_ARTIFACTS` environment variable.
pub fn artifacts_dir() -> PathBuf {
    std::env::var_os("BLITZ_TEST_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/blitz-test-artifacts"))
}

fn bless_snapshots() -> bool {
    std::env::var_os("BLITZ_TEST_BLESS").is_some_and(|v| v != "0")
}

impl<D: Document> Harness<D> {
    /// Render the document to an RGBA screenshot using the CPU renderer.
    ///
    /// The image is rendered at the viewport's physical size on a white background.
    pub fn screenshot(&mut self) -> Screenshot {
        self.pump();
        let mut doc = self.doc.inner_mut();
        let (width, height) = doc.get_viewport().window_size;
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
                paint_scene(scene, &mut doc, scale, width, height, 0, 0);
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

    /// Render the document and save it as a PNG at `path`
    pub fn save_screenshot(&mut self, path: impl AsRef<Path>) {
        self.screenshot().save_png(path);
    }

    /// Render the document and assert that it matches the reference PNG at `reference`.
    ///
    /// - If the reference image doesn't exist (or the `BLITZ_TEST_BLESS` env var is set),
    ///   the rendered image is written to `reference` instead.
    /// - On mismatch, the actual image, a diff image (differing pixels in red), and a DOM
    ///   dump are written to [`artifacts_dir`] and the assertion panics with their paths.
    pub fn assert_screenshot_matches(&mut self, reference: impl AsRef<Path>) {
        let reference = reference.as_ref();
        let actual = self.screenshot();

        if bless_snapshots() || !reference.exists() {
            actual.save_png(reference);
            println!("wrote reference image {}", reference.display());
            return;
        }

        let expected = Screenshot::load_png(reference).unwrap();
        let diff = compare_screenshots(&actual, &expected, 0);
        let failure = match &diff {
            Ok(None) => return,
            Ok(Some(diff)) => format!("{} differing pixels", diff.differing_pixels),
            Err(err) => err.clone(),
        };

        let stem = reference
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "screenshot".to_string());
        let dir = artifacts_dir();
        let actual_path = dir.join(format!("{stem}-actual.png"));
        actual.save_png(&actual_path);
        let mut artifact_list = format!("  actual: {}", actual_path.display());
        if let Ok(Some(diff)) = &diff {
            let diff_path = dir.join(format!("{stem}-diff.png"));
            diff.diff_image.save_png(&diff_path);
            artifact_list.push_str(&format!("\n  diff:   {}", diff_path.display()));
        }
        let dom_path = dir.join(format!("{stem}-dom.txt"));
        std::fs::write(&dom_path, self.dom_string()).unwrap();
        artifact_list.push_str(&format!("\n  dom:    {}", dom_path.display()));

        panic!(
            "screenshot does not match reference {} ({failure})\nartifacts:\n{artifact_list}\n\
             (set BLITZ_TEST_BLESS=1 to update the reference)",
            reference.display()
        );
    }
}
