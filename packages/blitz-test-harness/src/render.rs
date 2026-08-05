//! Screenshot comparison and snapshot assertions with on-disk failure artifacts.
//!
//! CPU rendering itself (the [`Screenshot`] type) lives in [`blitz_headless`].

use std::path::{Path, PathBuf};

use blitz_dom::Document;
use blitz_headless::{Screenshot, compare_screenshots, screenshot_document};

use crate::Harness;

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
        screenshot_document(&mut self.doc.inner_mut())
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
