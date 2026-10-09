//! Tests for the harness's CPU rendering, screenshot comparison, and snapshot assertions.

use blitz_test_harness::{Harness, HarnessOptions, Screenshot, compare_screenshots};

fn red_square_harness() -> Harness {
    Harness::from_html_with(
        r#"<html><body style="margin:0">
            <div style="width:50px; height:50px; background:rgb(255,0,0);"></div>
        </body></html>"#,
        HarnessOptions {
            width: 100,
            height: 100,
            ..Default::default()
        },
    )
}

#[test]
fn screenshot_renders_pixels() {
    let mut harness = red_square_harness();
    let shot = harness.screenshot();

    assert_eq!(shot.width, 100);
    assert_eq!(shot.height, 100);
    assert_eq!(shot.pixel(25, 25), [255, 0, 0, 255]);
    assert_eq!(shot.pixel(75, 75), [255, 255, 255, 255]);
}

#[test]
fn screenshot_png_roundtrip() {
    let mut harness = red_square_harness();
    let shot = harness.screenshot();

    let dir = std::env::temp_dir().join("blitz-test-harness-png-roundtrip");
    let path = dir.join("red-square.png");
    shot.save_png(&path);
    let loaded = Screenshot::load_png(&path).unwrap();
    assert_eq!(shot, loaded);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn compare_identical_screenshots() {
    let mut harness = red_square_harness();
    let a = harness.screenshot();
    let b = harness.screenshot();
    assert!(compare_screenshots(&a, &b, 0).unwrap().is_none());
}

#[test]
fn compare_differing_screenshots() {
    let mut harness = red_square_harness();
    let a = harness.screenshot();
    let mut b = a.clone();
    // Flip one pixel to green
    b.data[0..4].copy_from_slice(&[0, 255, 0, 255]);

    let diff = compare_screenshots(&a, &b, 0).unwrap().unwrap();
    assert_eq!(diff.differing_pixels, 1);
    assert_eq!(diff.diff_image.pixel(0, 0), [255, 0, 0, 255]);

    // Size mismatch is an error
    let small = Screenshot {
        width: 1,
        height: 1,
        data: vec![0; 4],
    };
    assert!(compare_screenshots(&a, &small, 0).is_err());
}

#[test]
fn snapshot_assertion_creates_and_matches_reference() {
    let dir = std::env::temp_dir().join("blitz-test-harness-snapshot-test");
    std::fs::remove_dir_all(&dir).ok();
    let reference = dir.join("red-square.png");

    let mut harness = red_square_harness();
    // First call writes the missing reference
    harness.assert_screenshot_matches(&reference);
    assert!(reference.exists());
    // Second call compares against it and passes
    harness.assert_screenshot_matches(&reference);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn snapshot_assertion_fails_with_artifacts_on_mismatch() {
    let dir = std::env::temp_dir().join("blitz-test-harness-snapshot-mismatch");
    std::fs::remove_dir_all(&dir).ok();
    let reference = dir.join("mismatch.png");

    // Write a reference that doesn't match the actual render
    let mut harness = red_square_harness();
    let mut wrong = harness.screenshot();
    for px in wrong.data.chunks_exact_mut(4) {
        px.copy_from_slice(&[0, 0, 255, 255]);
    }
    wrong.save_png(&reference);

    // SAFETY: test env var mutation; tests using BLITZ_TEST_ARTIFACTS run in this process only
    unsafe { std::env::set_var("BLITZ_TEST_ARTIFACTS", dir.join("artifacts")) };
    let result = std::panic::catch_unwind(move || {
        let mut harness = red_square_harness();
        harness.assert_screenshot_matches(&reference);
    });
    assert!(result.is_err(), "expected snapshot assertion to panic");

    let artifacts = dir.join("artifacts");
    assert!(artifacts.join("mismatch-actual.png").exists());
    assert!(artifacts.join("mismatch-diff.png").exists());
    assert!(artifacts.join("mismatch-dom.txt").exists());
    std::fs::remove_dir_all(&dir).ok();
}
