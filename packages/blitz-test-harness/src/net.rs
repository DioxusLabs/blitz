//! Deterministic network providers for tests.
//!
//! - [`FileNetProvider`]: serves requests from a local fixture directory (plus `data:`
//!   and `file:` URLs), synchronously and offline.
//! - [`RecordReplayProvider`]: records responses fetched through a real provider to a
//!   cache directory, then replays them offline on subsequent runs.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};

/// Counts of requests processed by a test net provider
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RequestCounts {
    pub succeeded: usize,
    pub failed: usize,
}

impl RequestCounts {
    pub fn total(&self) -> usize {
        self.succeeded + self.failed
    }
}

#[derive(Default)]
struct RequestLog {
    succeeded: AtomicUsize,
    failed: Mutex<Vec<String>>,
}

impl RequestLog {
    fn record_success(&self) {
        self.succeeded.fetch_add(1, Ordering::SeqCst);
    }

    fn record_failure(&self, url: &str, reason: &str) {
        eprintln!("Error loading {url}: {reason}");
        self.failed
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(url.to_string());
    }

    fn counts(&self) -> RequestCounts {
        RequestCounts {
            succeeded: self.succeeded.load(Ordering::SeqCst),
            failed: self
                .failed
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .len(),
        }
    }

    fn failed_urls(&self) -> Vec<String> {
        self.failed
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }
}

/// Decode a `data:` URL to bytes
pub fn load_data_url(url: &str) -> Result<Vec<u8>, String> {
    let data_url = data_url::DataUrl::process(url).map_err(|err| format!("{err:?}"))?;
    let decoded = data_url.decode_to_vec().map_err(|err| format!("{err:?}"))?;
    Ok(decoded.0)
}

/// Load the bytes for a request from local files:
///
/// - `data:` URLs are decoded directly.
/// - `file:` URLs are read from the filesystem.
/// - All other URLs are resolved by joining their path onto `base_dir`
///   (a URL path of `/style.css` maps to `<base_dir>/style.css`).
pub fn load_fixture_bytes(base_dir: &Path, request: &Request) -> Result<Vec<u8>, String> {
    match request.url.scheme() {
        "data" => load_data_url(request.url.as_str()),
        "file" => std::fs::read(request.url.path()).map_err(|err| format!("read file url: {err}")),
        _ => {
            let relative_path = request
                .url
                .path()
                .strip_prefix('/')
                .unwrap_or(request.url.path());
            let path = base_dir.join(relative_path);
            std::fs::read(&path).map_err(|err| format!("read {}: {err}", path.display()))
        }
    }
}

/// A synchronous [`NetProvider`] which serves requests from local files.
///
/// - `data:` URLs are decoded directly.
/// - `file:` URLs are read from the filesystem.
/// - All other URLs are resolved by joining their path onto the fixture directory
///   (a URL path of `/style.css` maps to `<base_dir>/style.css`).
///
/// All requests complete synchronously during `fetch`, so no idle-waiting is required:
/// a single [`Harness::pump`](crate::Harness::pump) after construction sees all resources.
pub struct FileNetProvider {
    base_dir: PathBuf,
    log: RequestLog,
}

impl FileNetProvider {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            log: RequestLog::default(),
        }
    }

    /// Counts of requests served/failed so far
    pub fn request_counts(&self) -> RequestCounts {
        self.log.counts()
    }

    /// URLs of requests which failed to load
    pub fn failed_urls(&self) -> Vec<String> {
        self.log.failed_urls()
    }
}

impl NetProvider for FileNetProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.to_string();
        match load_fixture_bytes(&self.base_dir, &request) {
            Ok(bytes) => {
                handler.bytes(url, Bytes::from(bytes));
                self.log.record_success();
            }
            Err(reason) => self.log.record_failure(&url, &reason),
        }
    }
}

/// Whether a [`RecordReplayProvider`] fetches through the inner provider (recording
/// responses to disk) or serves recorded responses from disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordReplayMode {
    Record,
    Replay,
}

/// A [`NetProvider`] which records responses to a cache directory in `Record` mode
/// (fetching through an inner provider, e.g. `blitz_net::Provider`), and serves them
/// back from the cache in `Replay` mode — making network-dependent tests deterministic
/// and offline-safe after a single recording run.
///
/// Responses are keyed by request URL.
pub struct RecordReplayProvider {
    cache_dir: PathBuf,
    mode: RecordReplayMode,
    inner: Option<Arc<dyn NetProvider>>,
    log: Arc<RequestLog>,
}

impl RecordReplayProvider {
    /// Create a provider in `Record` mode: requests are forwarded to `inner` and
    /// responses are written to `cache_dir`.
    pub fn record(cache_dir: impl Into<PathBuf>, inner: Arc<dyn NetProvider>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            mode: RecordReplayMode::Record,
            inner: Some(inner),
            log: Arc::new(RequestLog::default()),
        }
    }

    /// Create a provider in `Replay` mode: requests are served from `cache_dir` and
    /// fail if no recording exists.
    pub fn replay(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            mode: RecordReplayMode::Replay,
            inner: None,
            log: Arc::new(RequestLog::default()),
        }
    }

    pub fn mode(&self) -> RecordReplayMode {
        self.mode
    }

    /// Counts of requests served/failed so far
    pub fn request_counts(&self) -> RequestCounts {
        self.log.counts()
    }

    /// URLs of requests which failed to load
    pub fn failed_urls(&self) -> Vec<String> {
        self.log.failed_urls()
    }

    /// The file a recording for `url` is stored at
    pub fn cache_path_for_url(&self, url: &str) -> PathBuf {
        cache_path_for_url(&self.cache_dir, url)
    }
}

fn cache_path_for_url(cache_dir: &Path, url: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let hash = hasher.finish();

    // Include a sanitized suffix of the URL in the filename for debuggability
    let readable: String = url
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .rev()
        .take(48)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    cache_dir.join(format!("{hash:016x}-{readable}"))
}

struct RecordingHandler {
    inner: Box<dyn NetHandler>,
    cache_path: PathBuf,
    log: Arc<RequestLog>,
}

impl NetHandler for RecordingHandler {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        if let Some(parent) = self.cache_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        match std::fs::write(&self.cache_path, &bytes) {
            Ok(()) => self.log.record_success(),
            Err(err) => self
                .log
                .record_failure(&resolved_url, &format!("write recording: {err}")),
        }
        self.inner.bytes(resolved_url, bytes);
    }
}

impl NetProvider for RecordReplayProvider {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.to_string();

        // data: URLs need no recording: they are self-contained
        if request.url.scheme() == "data" {
            match load_data_url(request.url.as_str()) {
                Ok(bytes) => {
                    handler.bytes(url, Bytes::from(bytes));
                    self.log.record_success();
                }
                Err(reason) => self.log.record_failure(&url, &reason),
            }
            return;
        }

        let cache_path = self.cache_path_for_url(&url);
        match self.mode {
            RecordReplayMode::Record => {
                let inner = self.inner.as_ref().expect("record mode has an inner");
                let handler = Box::new(RecordingHandler {
                    inner: handler,
                    cache_path,
                    log: Arc::clone(&self.log),
                });
                inner.fetch(doc_id, request, handler);
            }
            RecordReplayMode::Replay => match std::fs::read(&cache_path) {
                Ok(bytes) => {
                    handler.bytes(url, Bytes::from(bytes));
                    self.log.record_success();
                }
                Err(err) => self.log.record_failure(
                    &url,
                    &format!("no recording at {}: {err}", cache_path.display()),
                ),
            },
        }
    }
}
