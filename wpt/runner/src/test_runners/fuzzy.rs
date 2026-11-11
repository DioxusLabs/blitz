//! Fuzzy reftest matching: tolerances from `<meta name=fuzzy>` (as parsed by the WPT
//! manifest) for reftest image comparison. See
//! <https://web-platform-tests.org/writing-tests/reftests.html#fuzzy-matching>

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FuzzyRange {
    pub min: u64,
    pub max: u64,
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

    fn range(min: u64, max: u64) -> FuzzyRange {
        FuzzyRange { min, max }
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
