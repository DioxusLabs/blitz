//! Tests for the harness's deterministic network providers.

use std::sync::Arc;

use blitz_test_harness::{FileNetProvider, Harness, HarnessOptions, RecordReplayProvider};

fn fixture_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("blitz-test-harness-net-{name}"));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const HTML: &str = r#"<html>
    <head><link rel="stylesheet" href="/style.css"></head>
    <body style="margin:0"><div id="box">box</div></body>
</html>"#;

const CSS: &str = "#box { width: 123px; height: 45px; }";

fn harness_with_provider(net_provider: Arc<dyn blitz_traits::net::NetProvider>) -> Harness {
    Harness::from_html_with(
        HTML,
        HarnessOptions {
            base_url: Some("http://example.test/index.html".to_string()),
            net_provider: Some(net_provider),
            ..Default::default()
        },
    )
}

#[test]
fn file_net_provider_serves_stylesheet_fixtures() {
    let dir = fixture_dir("fixtures");
    std::fs::write(dir.join("style.css"), CSS).unwrap();

    let provider = Arc::new(FileNetProvider::new(&dir));
    let mut harness = harness_with_provider(provider.clone());
    harness.pump();

    let rect = harness.layout_rect("#box");
    assert_eq!(rect.width, 123.0);
    assert_eq!(rect.height, 45.0);
    assert_eq!(provider.request_counts().succeeded, 1);
    assert_eq!(provider.request_counts().failed, 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn file_net_provider_records_failures() {
    let dir = fixture_dir("missing");
    let provider = Arc::new(FileNetProvider::new(&dir));
    let mut harness = harness_with_provider(provider.clone());
    harness.pump();

    assert_eq!(provider.request_counts().failed, 1);
    assert_eq!(
        provider.failed_urls(),
        vec!["http://example.test/style.css".to_string()]
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn file_net_provider_decodes_data_urls() {
    let html = r#"<html>
        <head><link rel="stylesheet" href="data:text/css;base64,I2JveCB7IHdpZHRoOiA3N3B4OyB9"></head>
        <body style="margin:0"><div id="box">box</div></body>
    </html>"#;

    let dir = fixture_dir("data-url");
    let provider = Arc::new(FileNetProvider::new(&dir));
    let mut harness = Harness::from_html_with(
        html,
        HarnessOptions {
            net_provider: Some(provider.clone()),
            ..Default::default()
        },
    );
    harness.pump();

    assert_eq!(harness.layout_rect("#box").width, 77.0);
    assert_eq!(provider.request_counts().succeeded, 1);
}

#[test]
fn record_replay_provider_roundtrip() {
    let fixtures = fixture_dir("rr-fixtures");
    std::fs::write(fixtures.join("style.css"), CSS).unwrap();
    let cache = fixture_dir("rr-cache");

    // Record: fetch through the file provider, recording responses into the cache
    let inner = Arc::new(FileNetProvider::new(&fixtures));
    let recorder = Arc::new(RecordReplayProvider::record(&cache, inner));
    let mut harness = harness_with_provider(recorder.clone());
    harness.pump();
    assert_eq!(harness.layout_rect("#box").width, 123.0);
    assert_eq!(recorder.request_counts().succeeded, 1);
    assert!(
        recorder
            .cache_path_for_url("http://example.test/style.css")
            .exists()
    );

    // Delete the fixtures: replay must work entirely from the cache
    std::fs::remove_dir_all(&fixtures).ok();

    let replayer = Arc::new(RecordReplayProvider::replay(&cache));
    let mut harness = harness_with_provider(replayer.clone());
    harness.pump();
    assert_eq!(harness.layout_rect("#box").width, 123.0);
    assert_eq!(replayer.request_counts().succeeded, 1);

    std::fs::remove_dir_all(&cache).ok();
}

#[test]
fn record_replay_provider_fails_on_missing_recording() {
    let cache = fixture_dir("rr-missing");
    let replayer = Arc::new(RecordReplayProvider::replay(&cache));
    let mut harness = harness_with_provider(replayer.clone());
    harness.pump();

    assert_eq!(replayer.request_counts().failed, 1);
    assert_eq!(
        replayer.failed_urls(),
        vec!["http://example.test/style.css".to_string()]
    );
    std::fs::remove_dir_all(&cache).ok();
}
