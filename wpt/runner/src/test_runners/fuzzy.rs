//! Extraction of `<meta name=fuzzy>` tags, which specify tolerances for reftest image
//! comparison. See <https://web-platform-tests.org/writing-tests/reftests.html#fuzzy-matching>
//!
//! The content values are parsed by the `wpt-runner-types` crate; this module scrapes
//! them out of the test's HTML and implements the image comparison.

use regex::Regex;
use std::sync::LazyLock;

pub use wpt_runner_types::fuzzy::{FuzzySpec, tolerance_for_reference};

static META_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<meta\s[^>]*>"#).unwrap());
static NAME_FUZZY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"name\s*=\s*['"]?fuzzy['"]?"#).unwrap());
static CONTENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"content\s*=\s*(?:['"]([^'"]*)['"]|([^\s'">]+))"#).unwrap());

/// Parses all `<meta name=fuzzy>` tags in the document
pub fn parse_fuzzy_metas(html: &str) -> Vec<FuzzySpec> {
    META_RE
        .find_iter(html)
        .filter_map(|tag| {
            let tag = tag.as_str();
            if !NAME_FUZZY_RE.is_match(tag) {
                return None;
            }
            let content = CONTENT_RE
                .captures(tag)
                .and_then(|c| c.get(1).or(c.get(2)))?;
            content.as_str().parse().ok()
        })
        .collect()
}

/// Compares two RGBA buffers, returning the maximum per-channel difference and the
/// number of pixels which differ in any channel.
pub fn fuzzy_buffer_diff(test_buffer: &[u8], ref_buffer: &[u8]) -> (u64, u64) {
    let mut max_difference: u64 = 0;
    let mut differing_pixels: u64 = 0;

    for (test_px, ref_px) in test_buffer
        .as_chunks::<4>()
        .0
        .iter()
        .zip(ref_buffer.as_chunks::<4>().0.iter())
    {
        let mut pixel_differs = false;
        for (t, r) in test_px.iter().zip(ref_px.iter()) {
            let diff = t.abs_diff(*r) as u64;
            if diff > 0 {
                pixel_differs = true;
                max_difference = max_difference.max(diff);
            }
        }
        differing_pixels += pixel_differs as u64;
    }

    (max_difference, differing_pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_metas_from_html() {
        let html = r#"
            <meta charset="utf-8">
            <meta name="fuzzy" content="maxDifference=0-5; totalPixels=0-100">
            <meta content="ref.html:maxDifference=1;totalPixels=2" name=fuzzy>
        "#;
        let specs = parse_fuzzy_metas(html);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].reference, None);
        assert_eq!(specs[0].tolerance.max_difference.max, 5);
        assert_eq!(specs[1].reference.as_deref(), Some("ref.html"));
        assert_eq!(specs[1].tolerance.max_difference.max, 1);
    }

    #[test]
    fn buffer_diff() {
        let a = [0u8, 0, 0, 255, 10, 10, 10, 255];
        let b = [0u8, 0, 0, 255, 10, 13, 10, 255];
        assert_eq!(fuzzy_buffer_diff(&a, &b), (3, 1));
        assert_eq!(fuzzy_buffer_diff(&a, &a), (0, 0));
    }
}
