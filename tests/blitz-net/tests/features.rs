mod common;

#[cfg(feature = "multipart")]
mod multipart_tests {
    use crate::common::{make_url, mount_post_ok, write_tempfile};
    use blitz_net::Provider;
    use blitz_traits::net::{Body, Entry, EntryValue, FormData, Request};
    use wiremock::MockServer;

    #[tokio::test]
    async fn body_multipart_sends_parts() {
        let server = MockServer::start().await;
        mount_post_ok(&server).await;

        let url = make_url(&server.uri());
        let form = FormData(vec![Entry {
            name: "field".to_string(),
            value: EntryValue::String("hello".to_string()),
        }]);
        let mut req = Request::get(url);
        req.method = http::Method::POST;
        req.body = Body::Form(form);
        req.content_type = Some("multipart/form-data".to_string());

        let provider = Provider::new(None);
        provider
            .fetch_async(req)
            .await
            .expect("multipart POST should succeed");

        let requests = server.received_requests().await.unwrap();
        let ct = requests[0]
            .headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(
            ct.starts_with("multipart/form-data"),
            "content-type should be multipart/form-data, got: {ct}"
        );
        let body_str = std::str::from_utf8(&requests[0].body).unwrap_or_default();
        assert!(
            body_str.contains("hello"),
            "multipart body should contain field value"
        );
    }

    #[tokio::test]
    async fn body_multipart_file_part_reads_from_disk() {
        let tmp = write_tempfile(b"file part contents");
        let path = tmp.path().to_path_buf();

        let server = MockServer::start().await;
        mount_post_ok(&server).await;

        let url = make_url(&server.uri());
        let form = FormData(vec![Entry {
            name: "upload".to_string(),
            value: EntryValue::File(path),
        }]);
        let mut req = Request::get(url);
        req.method = http::Method::POST;
        req.body = Body::Form(form);
        req.content_type = Some("multipart/form-data".to_string());

        let provider = Provider::new(None);
        provider
            .fetch_async(req)
            .await
            .expect("multipart file POST should succeed");

        let requests = server.received_requests().await.unwrap();
        let body_str = std::str::from_utf8(&requests[0].body).unwrap_or_default();
        assert!(
            body_str.contains("file part contents"),
            "multipart body should contain file contents"
        );
    }
}

#[cfg(feature = "cache")]
mod cache_tests {
    use crate::common::make_url;
    use blitz_net::Provider;
    use blitz_traits::net::Request;
    use std::sync::OnceLock;
    use tokio::sync::Mutex as AsyncMutex;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// `blitz_net::Provider`'s cache lives in a fixed on-disk directory shared
    /// across all `Provider` instances in-process. Serialize cache-touching tests
    /// so concurrent tests don't observe partially-cleared state.
    fn cache_lock() -> &'static AsyncMutex<()> {
        static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| AsyncMutex::new(()))
    }

    #[tokio::test]
    async fn clear_cache_succeeds_on_fresh_provider() {
        let _guard = cache_lock().lock().await;
        let provider = Provider::new(None);
        provider.clear_cache().await;
    }

    #[tokio::test]
    async fn second_fetch_served_from_cache_and_clear_cache_invalidates_entries() {
        let _guard = cache_lock().lock().await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("cache-control", "max-age=3600")
                    .set_body_string("cached"),
            )
            .mount(&server)
            .await;

        let url = make_url(&server.uri());
        let provider = Provider::new(None);
        provider.clear_cache().await;

        let (_, b1) = provider
            .fetch_async(Request::get(url.clone()))
            .await
            .expect("first fetch should succeed");
        assert_eq!(b1.as_ref(), b"cached");

        let (_, b2) = provider
            .fetch_async(Request::get(url.clone()))
            .await
            .expect("second fetch should succeed");
        assert_eq!(b2.as_ref(), b"cached");

        let count_after_two = server.received_requests().await.unwrap().len();
        assert_eq!(
            count_after_two, 1,
            "second request should be served from cache"
        );

        provider.clear_cache().await;

        provider
            .fetch_async(Request::get(url))
            .await
            .expect("fetch after clear");

        let count_after_clear = server.received_requests().await.unwrap().len();
        assert_eq!(
            count_after_clear, 2,
            "after clearing cache, next fetch should hit network"
        );
    }
}

#[cfg(feature = "cookies")]
mod cookie_tests {
    use crate::common::make_url;
    use blitz_net::Provider;
    use blitz_traits::net::Request;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn cookie_jar_persists_set_cookie_across_requests() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(wiremock::matchers::path("/set"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("set-cookie", "session=abc123; Path=/"),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(wiremock::matchers::path("/check"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        let base = server.uri();
        let provider = Provider::new(None);

        provider
            .fetch_async(Request::get(make_url(&format!("{base}/set"))))
            .await
            .expect("set-cookie request should succeed");

        provider
            .fetch_async(Request::get(make_url(&format!("{base}/check"))))
            .await
            .expect("cookie check request should succeed");

        let requests = server.received_requests().await.unwrap();
        let check_req = requests
            .iter()
            .find(|r| r.url.path() == "/check")
            .expect("should have /check request");
        let cookie_header = check_req
            .headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(
            cookie_header.contains("session=abc123"),
            "cookie should be sent on subsequent request, got: '{cookie_header}'"
        );
    }
}
