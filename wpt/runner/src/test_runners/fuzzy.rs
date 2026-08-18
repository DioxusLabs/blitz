//! Parsing of `<meta name=fuzzy>` tags, which specify tolerances for reftest image
//! comparison. See <https://web-platform-tests.org/writing-tests/reftests.html#fuzzy-matching>
//!
//! The `content` attribute has the form `[ <ref-name> ":" ] <fuzzy-value>` where
//! `<fuzzy-value>` is `maxDifference=<range>;totalPixels=<range>` (the key names are
//! optional, in which case the ranges are positional) and `<range>` is either a single
//! number or `<min>-<max>`.

use regex::Regex;
use std::sync::LazyLock;

static META_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<meta\s[^>]*>"#).unwrap());
static NAME_FUZZY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"name\s*=\s*['"]?fuzzy['"]?"#).unwrap());
static CONTENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"content\s*=\s*(?:['"]([^'"]*)['"]|([^\s'">]+))"#).unwrap());

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FuzzyRange {
    pub min: u64,
    pub max: u64,
}

impl FuzzyRange {
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (min, max) = match s.split_once('-') {
            Some((min, max)) => (min.trim().parse().ok()?, max.trim().parse().ok()?),
            None => {
                let value = s.parse().ok()?;
                (value, value)
            }
        };
        (min <= max).then_some(Self { min, max })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FuzzyTolerance {
    pub max_difference: FuzzyRange,
    pub total_pixels: FuzzyRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzySpec {
    /// Reference file this tolerance applies to (`None` = all references)
    pub reference: Option<String>,
    pub tolerance: FuzzyTolerance,
}

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
            parse_fuzzy_content(content.as_str())
        })
        .collect()
}

fn parse_fuzzy_content(content: &str) -> Option<FuzzySpec> {
    let content = content.trim();
    let (reference, value) = match content.split_once(':') {
        Some((prefix, rest)) if !prefix.contains('=') => (Some(prefix.trim().to_string()), rest),
        _ => (None, content),
    };

    let mut max_difference = None;
    let mut total_pixels = None;
    for (index, part) in value.split(';').enumerate() {
        let part = part.trim();
        if let Some(range) = part.strip_prefix("maxDifference=") {
            max_difference = FuzzyRange::parse(range);
        } else if let Some(range) = part.strip_prefix("totalPixels=") {
            total_pixels = FuzzyRange::parse(range);
        } else if index == 0 {
            max_difference = FuzzyRange::parse(part);
        } else {
            total_pixels = FuzzyRange::parse(part);
        }
    }

    Some(FuzzySpec {
        reference,
        tolerance: FuzzyTolerance {
            max_difference: max_difference?,
            total_pixels: total_pixels?,
        },
    })
}

/// Finds the tolerance applicable to `ref_file`: a spec naming that reference takes
/// precedence over an unnamed one.
pub fn tolerance_for_reference<'a>(
    specs: &'a [FuzzySpec],
    ref_file: &str,
) -> Option<&'a FuzzyTolerance> {
    specs
        .iter()
        .find(|spec| spec.reference.as_deref() == Some(ref_file))
        .or_else(|| specs.iter().find(|spec| spec.reference.is_none()))
        .map(|spec| &spec.tolerance)
}

/// Compares two RGBA buffers, returning the maximum per-channel difference and the
/// number of pixels which differ in any channel.
pub fn fuzzy_buffer_diff(test_buffer: &[u8], ref_buffer: &[u8]) -> (u64, u64) {
    let mut max_difference: u64 = 0;
    let mut differing_pixels: u64 = 0;

    for (test_px, ref_px) in test_buffer.chunks_exact(4).zip(ref_buffer.chunks_exact(4)) {
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

    fn range(min: u64, max: u64) -> FuzzyRange {
        FuzzyRange { min, max }
    }

    #[test]
    fn parse_named_ranges() {
        let spec = parse_fuzzy_content("maxDifference=0-2;totalPixels=0-40").unwrap();
        assert_eq!(spec.reference, None);
        assert_eq!(spec.tolerance.max_difference, range(0, 2));
        assert_eq!(spec.tolerance.total_pixels, range(0, 40));
    }

    #[test]
    fn parse_named_ranges_with_space() {
        let spec = parse_fuzzy_content("maxDifference=0-99; totalPixels=0-410").unwrap();
        assert_eq!(spec.tolerance.max_difference, range(0, 99));
        assert_eq!(spec.tolerance.total_pixels, range(0, 410));
    }

    #[test]
    fn parse_positional_ranges() {
        let spec = parse_fuzzy_content("2;40").unwrap();
        assert_eq!(spec.tolerance.max_difference, range(2, 2));
        assert_eq!(spec.tolerance.total_pixels, range(40, 40));
    }

    #[test]
    fn parse_per_reference() {
        let spec = parse_fuzzy_content("ref.html:maxDifference=0-2;totalPixels=0-40").unwrap();
        assert_eq!(spec.reference.as_deref(), Some("ref.html"));
        assert_eq!(spec.tolerance.max_difference, range(0, 2));
        assert_eq!(spec.tolerance.total_pixels, range(0, 40));
    }

    #[test]
    fn parse_invalid() {
        assert_eq!(parse_fuzzy_content("garbage"), None);
        assert_eq!(parse_fuzzy_content("maxDifference=0-2"), None);
    }

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
        assert_eq!(specs[1].reference.as_deref(), Some("ref.html"));
        assert_eq!(specs[1].tolerance.max_difference, range(1, 1));
    }

    #[test]
    fn tolerance_selection() {
        let specs = vec![
            FuzzySpec {
                reference: None,
                tolerance: FuzzyTolerance {
                    max_difference: range(0, 1),
                    total_pixels: range(0, 1),
                },
            },
            FuzzySpec {
                reference: Some("ref.html".to_string()),
                tolerance: FuzzyTolerance {
                    max_difference: range(0, 2),
                    total_pixels: range(0, 2),
                },
            },
        ];
        assert_eq!(
            tolerance_for_reference(&specs, "ref.html")
                .unwrap()
                .max_difference,
            range(0, 2)
        );
        assert_eq!(
            tolerance_for_reference(&specs, "other.html")
                .unwrap()
                .max_difference,
            range(0, 1)
        );
        assert_eq!(tolerance_for_reference(&[], "ref.html"), None);
    }

    #[test]
    fn buffer_diff() {
        let a = [0u8, 0, 0, 255, 10, 10, 10, 255];
        let b = [0u8, 0, 0, 255, 10, 13, 10, 255];
        assert_eq!(fuzzy_buffer_diff(&a, &b), (3, 1));
        assert_eq!(fuzzy_buffer_diff(&a, &a), (0, 0));
    }
}
