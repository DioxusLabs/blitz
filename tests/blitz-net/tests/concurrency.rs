mod common;

use blitz_net::Provider;
use blitz_traits::net::{AbortController, NetProvider, Request};
use common::{
    CaptureHandler, CaptureWaker, RESPONSE_DELAY, make_url, mount_get_ok, wait_for_received,
    wait_until,
};
use futures_util::future::join_all;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request as WireRequest, Respond, ResponseTemplate};

#[tokio::test]
async fn is_empty_true_on_fresh_provider() {
    let provider = Provider::new(None);
    assert!(provider.is_empty());
}

#[tokio::test]
async fn count_zero_on_fresh_provider() {
    let provider = Provider::new(None);
    assert_eq!(provider.count(), 0);
}

struct NotifyOnArrival {
    notify: Arc<Notify>,
    delay: Duration,
}

impl Respond for NotifyOnArrival {
    fn respond(&self, _req: &WireRequest) -> ResponseTemplate {
        self.notify.notify_one();
        ResponseTemplate::new(200).set_delay(self.delay)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn count_increments_during_in_flight_fetch() {
    let arrived = Arc::new(Notify::new());
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(NotifyOnArrival {
            notify: arrived.clone(),
            delay: RESPONSE_DELAY,
        })
        .mount(&server)
        .await;

    let waker = CaptureWaker::default();
    let provider = Provider::new(Some(Arc::new(waker.clone())));

    let url = make_url(&server.uri());
    let (tx, rx) = tokio::sync::oneshot::channel();
    NetProvider::fetch(
        &provider,
        1,
        Request::get(url),
        Box::new(CaptureHandler(tx)),
    );

    arrived.notified().await;
    assert_eq!(provider.count(), 1, "one in-flight fetch");
    assert!(!provider.is_empty());

    let _ = rx.await;
    assert!(
        wait_until(Duration::from_secs(1), || provider.is_empty()).await,
        "provider should become empty after fetch completes"
    );
    assert_eq!(provider.count(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn is_empty_returns_true_after_completion() {
    let server = MockServer::start().await;
    mount_get_ok(&server).await;

    let waker = CaptureWaker::default();
    let provider = Provider::new(Some(Arc::new(waker)));

    let url = make_url(&server.uri());
    let (tx, rx) = tokio::sync::oneshot::channel();
    NetProvider::fetch(
        &provider,
        1,
        Request::get(url),
        Box::new(CaptureHandler(tx)),
    );
    let _ = rx.await;

    assert!(
        wait_until(Duration::from_secs(1), || provider.is_empty()).await,
        "provider should become empty"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn per_host_limit_caps_concurrent_requests_at_six() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(RESPONSE_DELAY))
        .expect(8)
        .mount(&server)
        .await;

    let provider = Arc::new(Provider::new(None));
    let base_url = server.uri();

    let fetches_handle = tokio::spawn({
        let provider = provider.clone();
        async move {
            let futs: Vec<_> = (0..8)
                .map(|i| {
                    let p = provider.clone();
                    let url = make_url(&format!("{base_url}/?i={i}"));
                    async move { p.fetch_async(Request::get(url)).await }
                })
                .collect();
            join_all(futs).await
        }
    });

    // Each response is held for RESPONSE_DELAY, so no permit is released and no
    // 7th request can dispatch while we observe — the count we read is the cap,
    // not a racy snapshot.
    let observed = wait_for_received(&server, 6, Duration::from_secs(2)).await;
    assert_eq!(
        observed, 6,
        "per-host limit should cap in-flight at exactly 6"
    );

    let results = fetches_handle.await.unwrap();
    assert!(
        results.iter().all(|r| r.is_ok()),
        "all 8 fetches should succeed"
    );
    let received_total = server.received_requests().await.unwrap().len();
    assert_eq!(
        received_total, 8,
        "all 8 requests should eventually be served"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn per_host_limit_shared_across_ports_for_same_hostname() {
    // The semaphore is keyed by url.host_str(), so two mock servers on 127.0.0.1
    // with different ports share a single limiter — combined in-flight stays ≤ 6.
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(RESPONSE_DELAY))
        .mount(&server_a)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(RESPONSE_DELAY))
        .mount(&server_b)
        .await;

    let provider = Arc::new(Provider::new(None));
    let url_a = server_a.uri();
    let url_b = server_b.uri();

    let fetches_handle = tokio::spawn({
        let provider = provider.clone();
        async move {
            let urls: Vec<_> = (0..6)
                .map(|i| make_url(&format!("{url_a}/?i={i}")))
                .chain((0..6).map(|i| make_url(&format!("{url_b}/?i={i}"))))
                .collect();
            let futs: Vec<_> = urls
                .into_iter()
                .map(|u| {
                    let p = provider.clone();
                    async move { p.fetch_async(Request::get(u)).await }
                })
                .collect();
            join_all(futs).await
        }
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut combined = 0;
    while Instant::now() < deadline {
        let a = server_a.received_requests().await.unwrap().len();
        let b = server_b.received_requests().await.unwrap().len();
        combined = a + b;
        if combined >= 6 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        combined, 6,
        "same hostname → shared semaphore; combined in-flight should equal 6, got {combined}"
    );

    let results = fetches_handle.await.unwrap();
    assert!(
        results.iter().all(|r| r.is_ok()),
        "all fetches should succeed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn per_host_limit_not_shared_across_hostnames() {
    // Contrast to `per_host_limit_shared_across_ports_for_same_hostname`: the
    // same wiremock addressed via two host strings ("127.0.0.1" and "localhost")
    // gets two distinct semaphores, so combined in-flight can exceed 6.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(RESPONSE_DELAY))
        .mount(&server)
        .await;

    let port = server.address().port();
    let url_ip = format!("http://127.0.0.1:{port}");
    let url_local = format!("http://localhost:{port}");

    let provider = Arc::new(Provider::new(None));
    let fetches_handle = tokio::spawn({
        let provider = provider.clone();
        async move {
            let urls: Vec<_> = (0..6)
                .map(|i| make_url(&format!("{url_ip}/?i={i}")))
                .chain((0..6).map(|i| make_url(&format!("{url_local}/?i={i}"))))
                .collect();
            let futs: Vec<_> = urls
                .into_iter()
                .map(|u| {
                    let p = provider.clone();
                    async move { p.fetch_async(Request::get(u)).await }
                })
                .collect();
            join_all(futs).await
        }
    });

    let observed = wait_for_received(&server, 7, Duration::from_secs(2)).await;
    assert!(
        observed > 6,
        "different hostnames should not share a semaphore; only saw {observed} concurrent"
    );

    let results = fetches_handle.await.unwrap();
    assert!(
        results.iter().all(|r| r.is_ok()),
        "all fetches should succeed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn abort_before_dispatch_returns_abort_immediately() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(RESPONSE_DELAY))
        .mount(&server)
        .await;

    let waker = CaptureWaker::default();
    let provider = Provider::new(Some(Arc::new(waker.clone())));

    let controller = AbortController::default();
    let signal = controller.signal.clone();
    controller.abort();

    let url = make_url(&server.uri());
    let req = Request::get(url).signal(signal);
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    NetProvider::fetch(&provider, 77, req, Box::new(CaptureHandler(tx)));

    let waker_vec = waker.0.clone();
    assert!(
        wait_until(Duration::from_secs(2), || !waker_vec
            .lock()
            .unwrap()
            .is_empty())
        .await,
        "waker should fire even when pre-aborted"
    );

    let woken = waker_vec.lock().unwrap().clone();
    assert!(woken.contains(&77), "waker should fire with doc_id 77");
    assert!(
        rx.try_recv().is_err(),
        "handler should not be called when pre-aborted"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn abort_after_completion_is_noop() {
    let server = MockServer::start().await;
    mount_get_ok(&server).await;

    let waker = CaptureWaker::default();
    let provider = Provider::new(Some(Arc::new(waker)));

    let controller = AbortController::default();
    let signal = controller.signal.clone();

    let url = make_url(&server.uri());
    let req = Request::get(url).signal(signal);
    let (tx, rx) = tokio::sync::oneshot::channel();
    NetProvider::fetch(&provider, 88, req, Box::new(CaptureHandler(tx)));

    let _ = rx.await;
    controller.abort();

    assert!(
        wait_until(Duration::from_secs(1), || provider.is_empty()).await,
        "provider should become empty after fetch completes"
    );
    assert!(provider.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn same_signal_shared_across_requests() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(RESPONSE_DELAY))
        .mount(&server)
        .await;

    let waker = CaptureWaker::default();
    let provider = Provider::new(Some(Arc::new(waker.clone())));

    let controller = AbortController::default();
    let signal_a = controller.signal.clone();
    let signal_b = controller.signal.clone();

    let base = server.uri();
    let url_a = make_url(&format!("{base}/?r=a"));
    let url_b = make_url(&format!("{base}/?r=b"));

    let (tx_a, mut rx_a) = tokio::sync::oneshot::channel();
    let (tx_b, mut rx_b) = tokio::sync::oneshot::channel();

    NetProvider::fetch(
        &provider,
        91,
        Request::get(url_a).signal(signal_a),
        Box::new(CaptureHandler(tx_a)),
    );
    NetProvider::fetch(
        &provider,
        92,
        Request::get(url_b).signal(signal_b),
        Box::new(CaptureHandler(tx_b)),
    );

    controller.abort();

    let waker_vec = waker.0.clone();
    assert!(
        wait_until(Duration::from_secs(3), || {
            let v = waker_vec.lock().unwrap();
            v.contains(&91) && v.contains(&92)
        })
        .await,
        "both wakers should fire after shared-signal abort"
    );

    assert!(
        rx_a.try_recv().is_err(),
        "handler for request 91 should not be called"
    );
    assert!(
        rx_b.try_recv().is_err(),
        "handler for request 92 should not be called"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn abort_signal_aborted_mid_flight_returns_abort() {
    let arrived = Arc::new(Notify::new());
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(NotifyOnArrival {
            notify: arrived.clone(),
            delay: RESPONSE_DELAY,
        })
        .mount(&server)
        .await;

    let waker = CaptureWaker::default();
    let provider = Provider::new(Some(Arc::new(waker.clone())));

    let controller = AbortController::default();
    let signal = controller.signal.clone();

    let url = make_url(&server.uri());
    let req = Request::get(url).signal(signal);
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    // AbortFetch only wraps the NetProvider::fetch path, not fetch_async or fetch_with_callback.
    NetProvider::fetch(&provider, 55, req, Box::new(CaptureHandler(tx)));

    // Wait for the request to land at wiremock so we cancel a real in-flight
    // fetch rather than racing dispatch.
    arrived.notified().await;
    controller.abort();

    let waker_vec = waker.0.clone();
    assert!(
        wait_until(Duration::from_secs(2), || !waker_vec
            .lock()
            .unwrap()
            .is_empty())
        .await,
        "waker should fire after the aborted task completes"
    );

    let woken = waker_vec.lock().unwrap().clone();
    assert!(
        woken.contains(&55),
        "waker should fire with the right doc_id"
    );
    assert!(
        rx.try_recv().is_err(),
        "handler should not be called when aborted"
    );
}
