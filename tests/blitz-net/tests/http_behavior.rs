mod common;

use blitz_net::{Provider, ProviderError};
use blitz_traits::net::{Body, Entry, EntryValue, FormData, NetProvider, Request};
use bytes::Bytes;
use common::{
    CaptureHandler, CaptureWaker, make_url, mount_get_body, mount_get_ok, mount_get_status,
    mount_post_ok, wait_until,
};
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn injects_user_agent_header() {
    let server = MockServer::start().await;
    mount_get_ok(&server).await;

    let url = make_url(&server.uri());
    let provider = Provider::new(None);
    provider
        .fetch_async(Request::get(url))
        .await
        .expect("request should succeed");

    let requests = server.received_requests().await.unwrap();
    let ua = requests[0]
        .headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        ua.starts_with("Mozilla/5.0"),
        "user-agent should be set, got: '{ua}'"
    );
}

#[tokio::test]
async fn forwards_request_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::header("x-custom", "myvalue"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let url = make_url(&server.uri());
    let mut req = Request::get(url);
    req.headers
        .insert("x-custom", http::HeaderValue::from_static("myvalue"));

    let provider = Provider::new(None);
    let result = provider.fetch_async(req).await;
    assert!(result.is_ok(), "forwarded header should match: {result:?}");
}

#[tokio::test]
async fn forwards_method_post() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("posted"))
        .mount(&server)
        .await;

    let url = make_url(&server.uri());
    let mut req = Request::get(url);
    req.method = http::Method::POST;

    let provider = Provider::new(None);
    let (_, bytes) = provider
        .fetch_async(req)
        .await
        .expect("POST should succeed");
    assert_eq!(bytes.as_ref(), b"posted");
}

#[tokio::test]
async fn body_empty_sends_no_body() {
    let server = MockServer::start().await;
    mount_post_ok(&server).await;

    let url = make_url(&server.uri());
    let mut req = Request::get(url);
    req.method = http::Method::POST;
    req.body = Body::Empty;

    let provider = Provider::new(None);
    let result = provider.fetch_async(req).await;
    assert!(result.is_ok());

    let requests = server.received_requests().await.unwrap();
    let body = requests[0].body.clone();
    assert!(body.is_empty(), "empty body sends no bytes");
}

#[tokio::test]
async fn body_bytes_sends_raw_payload() {
    let server = MockServer::start().await;
    mount_post_ok(&server).await;

    let url = make_url(&server.uri());
    let mut req = Request::get(url);
    req.method = http::Method::POST;
    req.body = Body::Bytes(Bytes::from_static(b"raw payload"));

    let provider = Provider::new(None);
    provider
        .fetch_async(req)
        .await
        .expect("body bytes should send");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].body, b"raw payload");
}

#[tokio::test]
async fn body_form_urlencoded_sends_encoded() {
    let server = MockServer::start().await;
    mount_post_ok(&server).await;

    let url = make_url(&server.uri());
    let form = FormData(vec![Entry {
        name: "key".to_string(),
        value: EntryValue::String("value".to_string()),
    }]);
    let mut req = Request::get(url);
    req.method = http::Method::POST;
    req.body = Body::Form(form);
    req.content_type = Some("application/x-www-form-urlencoded".to_string());

    let provider = Provider::new(None);
    provider
        .fetch_async(req)
        .await
        .expect("url-encoded form should send");

    let requests = server.received_requests().await.unwrap();
    let body_str = std::str::from_utf8(&requests[0].body).unwrap();
    assert!(
        body_str.contains("key=value"),
        "body should contain encoded form: {body_str}"
    );
}

#[tokio::test]
async fn content_type_header_set_when_provided() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wiremock::matchers::header(
            "content-type",
            "application/json",
        ))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let url = make_url(&server.uri());
    let mut req = Request::get(url);
    req.method = http::Method::POST;
    req.content_type = Some("application/json".to_string());
    req.body = Body::Bytes(Bytes::from_static(b"{}"));

    let provider = Provider::new(None);
    let result = provider.fetch_async(req).await;
    assert!(
        result.is_ok(),
        "content-type header should be forwarded: {result:?}"
    );
}

#[tokio::test]
async fn fetch_with_callback_invokes_callback_on_ok() {
    let server = MockServer::start().await;
    mount_get_body(&server, "callback ok").await;

    let url = make_url(&server.uri());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let provider = Provider::new(None);
    provider.fetch_with_callback(
        Request::get(url),
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );

    let result = rx.await.expect("callback should be invoked");
    let (_, bytes) = result.expect("callback result should be ok");
    assert_eq!(bytes.as_ref(), b"callback ok");
}

#[tokio::test]
async fn fetch_with_callback_invokes_callback_on_404() {
    let server = MockServer::start().await;
    mount_get_status(&server, 404).await;

    let url = make_url(&server.uri());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let provider = Provider::new(None);
    provider.fetch_with_callback(
        Request::get(url),
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );

    let result = rx.await.expect("callback should be invoked");
    let err = result.expect_err("callback result should be error on 404");
    assert!(
        matches!(err, ProviderError::HttpStatus { status, .. } if status.as_u16() == 404),
        "expected HttpStatus 404, got: {err}"
    );
}

#[tokio::test]
async fn net_provider_trait_fetch_delivers_bytes_to_handler() {
    let server = MockServer::start().await;
    mount_get_body(&server, "handler bytes").await;

    let waker = CaptureWaker::default();
    let provider = Provider::new(Some(Arc::new(waker.clone())));

    let url = make_url(&server.uri());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handler = CaptureHandler(tx);

    NetProvider::fetch(&provider, 42, Request::get(url), Box::new(handler));

    let (_, bytes) = rx.await.expect("handler should be called");
    assert_eq!(bytes.as_ref(), b"handler bytes");
}

#[tokio::test]
async fn net_provider_trait_fetch_skips_handler_on_error() {
    let server = MockServer::start().await;
    mount_get_status(&server, 500).await;

    let waker = CaptureWaker::default();
    let provider = Provider::new(Some(Arc::new(waker.clone())));

    let url = make_url(&server.uri());
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let handler = CaptureHandler(tx);

    NetProvider::fetch(&provider, 99, Request::get(url), Box::new(handler));

    // The waker fires when the fetch task completes (success or error). Once it
    // has fired, the handler dispatch decision has been made — if the impl is
    // correct, the handler is skipped on error and rx remains empty.
    let waker_vec = waker.0.clone();
    assert!(
        wait_until(Duration::from_secs(2), || waker_vec
            .lock()
            .unwrap()
            .contains(&99))
        .await,
        "waker should fire after error"
    );

    assert!(
        rx.try_recv().is_err(),
        "handler should not be called on error"
    );
}

#[tokio::test]
async fn net_provider_trait_wakes_waker_with_doc_id() {
    let server = MockServer::start().await;
    mount_get_ok(&server).await;

    let waker = CaptureWaker::default();
    let provider = Provider::new(Some(Arc::new(waker.clone())));

    let url = make_url(&server.uri());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handler = CaptureHandler(tx);
    NetProvider::fetch(&provider, 77, Request::get(url), Box::new(handler));

    let _ = rx.await;

    let woken = waker.0.lock().unwrap().clone();
    assert!(woken.contains(&77), "waker should be called with doc_id=77");
}

#[tokio::test]
async fn shared_returns_arc_dyn_netprovider() {
    let provider: Arc<dyn NetProvider> = Provider::shared(None);
    let server = MockServer::start().await;
    mount_get_body(&server, "shared").await;

    let url = make_url(&server.uri());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handler = CaptureHandler(tx);
    provider.fetch(1, Request::get(url), Box::new(handler));

    rx.await.expect("shared provider should work");
}

// Display strings are part of blitz-net's public surface; variant identity is covered by
// schemes.rs. Asserting Display rather than variant also keeps the test stable under
// workspace feature unification (e.g., `cache` wrapping reqwest errors in
// reqwest-middleware when apps/browser pulls the feature in transitively).
#[tokio::test]
async fn provider_error_display_renders_each_variant() {
    let provider = Provider::new(None);

    let display = provider
        .fetch_async(Request::get(make_url("file:///nonexistent/path.txt")))
        .await
        .expect_err("missing file")
        .to_string();
    assert!(!display.is_empty(), "Io display empty");

    let display = provider
        .fetch_async(Request::get(make_url("data:not valid")))
        .await
        .expect_err("invalid data url")
        .to_string();
    assert!(!display.is_empty(), "DataUrl display empty");

    let display = provider
        .fetch_async(Request::get(make_url("data:text/plain;base64,!!!")))
        .await
        .expect_err("invalid base64")
        .to_string();
    assert!(!display.is_empty(), "DataUrlBase64 display empty");

    let display = provider
        .fetch_async(Request::get(make_url(
            "http://intentionally-nonexistent-host.invalid/",
        )))
        .await
        .expect_err("dns failure")
        .to_string();
    assert!(
        display.starts_with("reqwest error:") || display.starts_with("reqwest middleware error:"),
        "reqwest/middleware display: {display}"
    );

    let server = MockServer::start().await;
    mount_get_status(&server, 404).await;
    let display = provider
        .fetch_async(Request::get(make_url(&server.uri())))
        .await
        .expect_err("404")
        .to_string();
    assert!(display.contains("404"), "HttpStatus display: {display}");
    assert!(
        display.contains(&server.uri()),
        "HttpStatus display missing url: {display}"
    );

    assert!(ProviderError::Abort.to_string().contains("aborted"));
}
