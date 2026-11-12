//! Fuzzy reftest matching: compares renderings against the `<meta name=fuzzy>` tolerances
//! parsed by `wpt_manifest`. See
//! <https://web-platform-tests.org/writing-tests/reftests.html#fuzzy-matching>

use regex::Regex;
use std::sync::LazyLock;

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

/// Tolerance from a Blitz-specific `<meta name="blitz-fuzzy" content="<threshold>;<pixels>">`:
/// renderings match if at most `pixels` pixels differ by more than `threshold` in any
/// channel. Unlike WPT's fuzzy tolerance it ignores small differences entirely, which is
/// what comparing against another rasterizer's output needs (anti-aliased edges and
/// gradients differ slightly almost everywhere). Used by the generated resvg tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThresholdTolerance {
    pub threshold: u8,
    pub max_pixels: u64,
}

impl ThresholdTolerance {
    /// Panics on a malformed `content`, so a typo can't silently fall back to
    /// the default comparison.
    pub fn from_html(html: &str) -> Option<Self> {
        static META_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)<meta\s[^>]*>").unwrap());
        static ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"([\w-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#).unwrap()
        });
        static CONTENT_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^\s*(\d+)\s*;\s*(\d+)\s*$").unwrap());

        META_RE.find_iter(html).find_map(|meta| {
            let mut name = None;
            let mut content = None;
            for attr in ATTR_RE.captures_iter(meta.as_str()) {
                let value = attr
                    .get(2)
                    .or(attr.get(3))
                    .or(attr.get(4))
                    .unwrap()
                    .as_str();
                match attr[1].to_ascii_lowercase().as_str() {
                    "name" => name = Some(value),
                    "content" => content = Some(value),
                    _ => {}
                }
            }
            if name != Some("blitz-fuzzy") {
                return None;
            }
            let content = content.unwrap_or_default();
            let parsed = CONTENT_RE.captures(content).and_then(|caps| {
                Some(Self {
                    threshold: caps[1].parse().ok()?,
                    max_pixels: caps[2].parse().ok()?,
                })
            });
            Some(parsed.unwrap_or_else(|| {
                panic!(
                    "invalid blitz-fuzzy content {content:?}: expected <threshold 0-255>;<pixels>"
                )
            }))
        })
    }

    pub fn matches(self, test_buffer: &[u8], ref_buffer: &[u8]) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_tolerance() {
        let tolerance = ThresholdTolerance::from_html(
            "<link rel=match href=a-ref.html>\n<meta name=\"blitz-fuzzy\" content=\"64;1\">",
        )
        .unwrap();
        assert_eq!(
            tolerance,
            ThresholdTolerance {
                threshold: 64,
                max_pixels: 1
            }
        );
        let a = [0u8, 0, 0, 255, 0, 0, 0, 255];
        assert!(tolerance.matches(&a, &[64, 0, 0, 255, 200, 0, 0, 255]));
        assert!(!tolerance.matches(&a, &[65, 0, 0, 255, 200, 0, 0, 255]));
        assert_eq!(ThresholdTolerance::from_html("<p>"), None);
        assert_eq!(
            ThresholdTolerance::from_html("<META content='8; 20' NAME=blitz-fuzzy>"),
            Some(ThresholdTolerance {
                threshold: 8,
                max_pixels: 20
            })
        );
    }

    #[test]
    #[should_panic(expected = "invalid blitz-fuzzy content")]
    fn threshold_tolerance_out_of_range() {
        ThresholdTolerance::from_html("<meta name=blitz-fuzzy content=256;1>");
    }

    #[test]
    fn buffer_diff() {
        let a = [0u8, 0, 0, 255, 10, 10, 10, 255];
        let b = [0u8, 0, 0, 255, 10, 13, 10, 255];
        assert_eq!(fuzzy_buffer_diff(&a, &b), (3, 1));
        assert_eq!(fuzzy_buffer_diff(&a, &a), (0, 0));
    }
}
