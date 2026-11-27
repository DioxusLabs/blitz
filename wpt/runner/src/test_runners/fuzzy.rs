//! Fuzzy reftest matching: compares renderings against the `<meta name=fuzzy>` tolerances
//! parsed by `wpt_manifest`. See
//! <https://web-platform-tests.org/writing-tests/reftests.html#fuzzy-matching>

use wpt_manifest::FuzzyTolerance;

pub(super) fn within_fuzzy_tolerance(
    max_difference: u64,
    differing_pixels: u64,
    tolerance: &FuzzyTolerance,
) -> bool {
    let difference = tolerance.max_difference;
    let pixels = tolerance.total_pixels;
    // Match wptrunner: identical images are allowed if either range starts at zero.
    if max_difference == 0 && differing_pixels == 0 {
        return difference.min == 0 || pixels.min == 0;
    }
    (difference.min..=difference.max).contains(&max_difference)
        && (pixels.min..=pixels.max).contains(&differing_pixels)
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
    fn fuzzy_tolerance_checks_inclusive_bounds() {
        use wpt_manifest::FuzzyRange;

        let mut tolerance = FuzzyTolerance {
            max_difference: FuzzyRange { min: 10, max: 15 },
            total_pixels: FuzzyRange { min: 20, max: 30 },
        };
        assert!(within_fuzzy_tolerance(10, 20, &tolerance));
        assert!(within_fuzzy_tolerance(15, 30, &tolerance));
        assert!(!within_fuzzy_tolerance(9, 20, &tolerance));
        assert!(!within_fuzzy_tolerance(10, 19, &tolerance));
        assert!(!within_fuzzy_tolerance(16, 20, &tolerance));
        assert!(!within_fuzzy_tolerance(10, 31, &tolerance));
        assert!(!within_fuzzy_tolerance(0, 0, &tolerance));
        tolerance.total_pixels.min = 0;
        assert!(within_fuzzy_tolerance(0, 0, &tolerance));
    }

    #[test]
    fn buffer_diff() {
        let a = [0u8, 0, 0, 255, 10, 10, 10, 255];
        let b = [0u8, 0, 0, 255, 10, 13, 10, 255];
        assert_eq!(fuzzy_buffer_diff(&a, &b), (3, 1));
        assert_eq!(fuzzy_buffer_diff(&a, &a), (0, 0));
    }
}
