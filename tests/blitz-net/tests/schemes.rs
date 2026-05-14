mod common;

use blitz_net::{Provider, ProviderError};
use blitz_traits::net::Request;
use common::{make_url, mount_get_body, mount_get_status, write_tempfile};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn data_url_plain_decodes() {
    let provider = Provider::new(None);
    let url = make_url("data:text/plain,hello%20world");
    let (_, bytes) = provider
        .fetch_async(Request::get(url))
        .await
        .expect("data url should decode");
    assert_eq!(bytes.as_ref(), b"hello world");
}

#[tokio::test]
async fn data_url_base64_decodes() {
    let provider = Provider::new(None);
    let url = make_url("data:text/plain;base64,aGVsbG8=");
    let (_, bytes) = provider
        .fetch_async(Request::get(url))
        .await
        .expect("base64 data url should decode");
    assert_eq!(bytes.as_ref(), b"hello");
}

#[tokio::test]
async fn data_url_invalid_returns_data_url_error() {
    let provider = Provider::new(None);
    let url = make_url("data:not valid at all without comma");
    let err = provider
        .fetch_async(Request::get(url))
        .await
        .expect_err("invalid data url should error");
    assert!(
        matches!(err, ProviderError::DataUrl(_)),
        "expected DataUrl error, got: {err}"
    );
}

#[tokio::test]
async fn data_url_invalid_base64_returns_base64_error() {
    let provider = Provider::new(None);
    let url = make_url("data:text/plain;base64,!!!invalid!!!");
    let err = provider
        .fetch_async(Request::get(url))
        .await
        .expect_err("invalid base64 should error");
    assert!(
        matches!(err, ProviderError::DataUrlBase64(_)),
        "expected DataUrlBase64 error, got: {err}"
    );
}

#[tokio::test]
async fn file_url_reads_existing_file() {
    let tmp = write_tempfile(b"file content");
    let url = url::Url::from_file_path(tmp.path()).expect("path → file url");

    let provider = Provider::new(None);
    let (_, bytes) = provider
        .fetch_async(Request::get(url))
        .await
        .expect("existing file should be readable");
    assert_eq!(bytes.as_ref(), b"file content");
}

#[tokio::test]
async fn file_url_missing_returns_io_error() {
    let url = make_url("file:///nonexistent/path/that/does/not/exist.txt");
    let provider = Provider::new(None);
    let err = provider
        .fetch_async(Request::get(url))
        .await
        .expect_err("missing file should error");
    assert!(
        matches!(err, ProviderError::Io(_)),
        "expected Io error, got: {err}"
    );
}

#[tokio::test]
async fn http_200_returns_bytes_and_resolved_url() {
    let server = MockServer::start().await;
    mount_get_body(&server, "ok").await;

    let url = make_url(&server.uri());
    let provider = Provider::new(None);
    let (resolved, bytes) = provider
        .fetch_async(Request::get(url.clone()))
        .await
        .expect("200 should succeed");
    assert_eq!(bytes.as_ref(), b"ok");
    assert!(
        resolved.starts_with(&server.uri()),
        "resolved url should reference the mock server, got: {resolved}"
    );
}

#[tokio::test]
async fn http_404_returns_http_status_error() {
    let server = MockServer::start().await;
    mount_get_status(&server, 404).await;

    let url = make_url(&server.uri());
    let provider = Provider::new(None);
    let err = provider
        .fetch_async(Request::get(url))
        .await
        .expect_err("404 should error");
    assert!(
        matches!(err, ProviderError::HttpStatus { status, .. } if status.as_u16() == 404),
        "expected HttpStatus 404, got: {err}"
    );
}

#[tokio::test]
async fn http_500_returns_http_status_error() {
    let server = MockServer::start().await;
    mount_get_status(&server, 500).await;

    let url = make_url(&server.uri());
    let provider = Provider::new(None);
    let err = provider
        .fetch_async(Request::get(url))
        .await
        .expect_err("500 should error");
    assert!(
        matches!(err, ProviderError::HttpStatus { status, .. } if status.as_u16() == 500),
        "expected HttpStatus 500, got: {err}"
    );
}

#[tokio::test]
async fn http_redirect_resolved_url_is_final() {
    let server = MockServer::start().await;
    let final_url = format!("{}/final", server.uri());

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(301).insert_header("location", final_url.as_str()))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/final"))
        .respond_with(ResponseTemplate::new(200).set_body_string("final"))
        .mount(&server)
        .await;

    let url = make_url(&server.uri());
    let provider = Provider::new(None);
    let (resolved, bytes) = provider
        .fetch_async(Request::get(url))
        .await
        .expect("redirect should follow and succeed");
    assert_eq!(bytes.as_ref(), b"final");
    assert!(
        resolved.ends_with("/final"),
        "resolved url should end at /final, got: {resolved}"
    );
}

#[tokio::test]
async fn http_redirect_loop_returns_error() {
    let server = MockServer::start().await;
    let loop_url = format!("{}/loop", server.uri());
    let root_url = server.uri();

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", loop_url.as_str()))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/loop"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", root_url.as_str()))
        .mount(&server)
        .await;

    let url = make_url(&server.uri());
    let provider = Provider::new(None);
    let err = provider
        .fetch_async(Request::get(url))
        .await
        .expect_err("redirect loop should error");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("redirect") || msg.contains("too many"),
        "expected redirect loop error, got: {err}"
    );
}

#[tokio::test]
async fn http_307_preserves_post_method() {
    let server = MockServer::start().await;
    let end_url = format!("{}/end", server.uri());

    Mock::given(method("POST"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", end_url.as_str()))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/end"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let start_url = make_url(&format!("{}/start", server.uri()));
    let mut req = Request::get(start_url);
    req.method = http::Method::POST;

    let provider = Provider::new(None);
    let (_, bytes) = provider
        .fetch_async(req)
        .await
        .expect("307 POST should preserve method and succeed");
    assert_eq!(bytes.as_ref(), b"ok");
}

// 302 with POST: reqwest (following browser compat) converts to GET.
// This test pins the current behavior so any future change is visible.
#[tokio::test]
async fn http_302_post_redirect_method() {
    let server = MockServer::start().await;
    let end_url = format!("{}/end", server.uri());

    Mock::given(method("POST"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", end_url.as_str()))
        .mount(&server)
        .await;

    // reqwest converts POST→GET on 302; mount GET so the redirected request matches.
    Mock::given(method("GET"))
        .and(path("/end"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let start_url = make_url(&format!("{}/start", server.uri()));
    let mut req = Request::get(start_url);
    req.method = http::Method::POST;

    let provider = Provider::new(None);
    let (_, bytes) = provider
        .fetch_async(req)
        .await
        .expect("302 POST redirect should succeed after method conversion");
    assert_eq!(bytes.as_ref(), b"ok");
}
