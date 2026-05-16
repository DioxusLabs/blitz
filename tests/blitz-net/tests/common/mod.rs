// Each helper is used by at least one test binary but not by all of them, so per-item
// `#[allow(dead_code)]` is required — without it, every binary's compile would warn on
// the helpers it doesn't reference. Prefer the per-item form over a blanket file-level
// allow so genuinely unreferenced helpers still surface in review.
use blitz_traits::net::{Bytes, NetHandler, NetWaker};
use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

// 300ms is wide enough to keep mid-flight observation windows deterministic on slow CI.
#[allow(dead_code)]
pub const RESPONSE_DELAY: Duration = Duration::from_millis(300);

#[allow(dead_code)]
pub struct CaptureHandler(pub tokio::sync::oneshot::Sender<(String, Bytes)>);

impl NetHandler for CaptureHandler {
    fn bytes(self: Box<Self>, url: String, b: Bytes) {
        let _ = self.0.send((url, b));
    }
}

#[allow(dead_code)]
#[derive(Default, Clone)]
pub struct CaptureWaker(pub Arc<Mutex<Vec<usize>>>);

impl NetWaker for CaptureWaker {
    fn wake(&self, id: usize) {
        self.0.lock().unwrap().push(id);
    }
}

#[allow(dead_code)]
pub fn make_url(s: &str) -> url::Url {
    url::Url::parse(s).expect("valid url")
}

#[allow(dead_code)]
pub fn write_tempfile(contents: &[u8]) -> tempfile::NamedTempFile {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(contents).unwrap();
    tmp
}

#[allow(dead_code)]
pub async fn mount_get_ok(server: &MockServer) {
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;
}

#[allow(dead_code)]
pub async fn mount_get_body(server: &MockServer, body: &'static str) {
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(server)
        .await;
}

#[allow(dead_code)]
pub async fn mount_get_status(server: &MockServer, status: u16) {
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(status))
        .mount(server)
        .await;
}

#[allow(dead_code)]
pub async fn mount_post_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;
}

#[allow(dead_code)]
pub async fn wait_until<F: FnMut() -> bool>(timeout: Duration, mut condition: F) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    condition()
}

#[allow(dead_code)]
pub async fn wait_for_received(server: &MockServer, target: usize, timeout: Duration) -> usize {
    let deadline = Instant::now() + timeout;
    loop {
        let n = server.received_requests().await.unwrap().len();
        if n >= target || Instant::now() >= deadline {
            return n;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
