//! Fuzzy reftest matching: compares renderings against the `<meta name=fuzzy>` tolerances
//! parsed by `wpt_manifest`. See
//! <https://web-platform-tests.org/writing-tests/reftests.html#fuzzy-matching>

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
    fn buffer_diff() {
        let a = [0u8, 0, 0, 255, 10, 10, 10, 255];
        let b = [0u8, 0, 0, 255, 10, 13, 10, 255];
        assert_eq!(fuzzy_buffer_diff(&a, &b), (3, 1));
        assert_eq!(fuzzy_buffer_diff(&a, &a), (0, 0));
    }
}
