//! Network activity recording. While a CDP session has the `Network` domain
//! enabled, the [`NetProvider`] of each document it inspects is wrapped in a
//! [`RecordingNetProvider`], which logs each request and its outcome; the
//! server later translates the logged records into `Network` domain events.
//!
//! Requests a document makes while it is being built (before the server can
//! see it) are only recorded if the embedder creates it with a provider from
//! [`recording_net_provider`]. Embedders that fetch a document's source
//! themselves can report that fetch with a [`DocumentLoadRecorder`].
//!
//! `NetHandler` only reports the final URL and body of a successful response
//! (or an error message), so responses are reported with status 200 and no
//! headers, and their MIME type is guessed from the URL and body.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use blitz_traits::net::{AbortSignal, Bytes, NetHandler, NetProvider, Request};

use crate::DevtoolsWaker;

/// The CDP `Network.ResourceType` of a request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceType {
    Document,
    Stylesheet,
    Image,
    Font,
    Script,
    Other,
}

impl ResourceType {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ResourceType::Document => "Document",
            ResourceType::Stylesheet => "Stylesheet",
            ResourceType::Image => "Image",
            ResourceType::Font => "Font",
            ResourceType::Script => "Script",
            ResourceType::Other => "Other",
        }
    }

    fn from_mime_type(mime_type: &str) -> Self {
        match mime_type {
            "text/html" | "application/xhtml+xml" => ResourceType::Document,
            "text/css" => ResourceType::Stylesheet,
            "text/javascript" | "application/javascript" => ResourceType::Script,
            m if m.starts_with("image/") => ResourceType::Image,
            m if m.starts_with("font/") => ResourceType::Font,
            _ => ResourceType::Other,
        }
    }
}

/// A request was issued (`Network.requestWillBeSent`)
pub(crate) struct RequestWillBeSent {
    pub(crate) request_id: u64,
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) resource_type: ResourceType,
    pub(crate) timestamp: f64,
    pub(crate) wall_time: f64,
}

/// A request completed successfully (`Network.responseReceived`,
/// `Network.dataReceived` and `Network.loadingFinished`)
pub(crate) struct ResponseReceived {
    pub(crate) request_id: u64,
    pub(crate) url: String,
    pub(crate) mime_type: &'static str,
    pub(crate) resource_type: ResourceType,
    pub(crate) body: Bytes,
    pub(crate) timestamp: f64,
}

/// A request failed or was cancelled (`Network.loadingFailed`)
pub(crate) struct LoadingFailed {
    pub(crate) request_id: u64,
    pub(crate) resource_type: ResourceType,
    pub(crate) error_text: String,
    pub(crate) canceled: bool,
    pub(crate) timestamp: f64,
}

pub(crate) enum NetworkRecord {
    RequestWillBeSent(RequestWillBeSent),
    ResponseReceived(ResponseReceived),
    LoadingFailed(LoadingFailed),
}

impl NetworkRecord {
    pub(crate) fn timestamp(&self) -> f64 {
        match self {
            NetworkRecord::RequestWillBeSent(record) => record.timestamp,
            NetworkRecord::ResponseReceived(record) => record.timestamp,
            NetworkRecord::LoadingFailed(record) => record.timestamp,
        }
    }
}

/// Network records logged for all recorded documents (keyed by the id of
/// the document that issued the request)
pub(crate) struct NetworkLog {
    records: Mutex<Vec<(usize, NetworkRecord)>>,
    /// Whether any session has the `Network` domain enabled. Providers
    /// wrapped while recording (e.g. inherited by sub-documents created in
    /// the meantime) stop recording when this is cleared.
    active: AtomicBool,
    /// Whether the embedder has been woken for records not yet taken
    woken: AtomicBool,
    next_request_id: AtomicU64,
    waker: Arc<dyn DevtoolsWaker>,
}

/// The log of the most recently created server, which providers created by
/// [`recording_net_provider`] record into
static CURRENT_LOG: Mutex<Weak<NetworkLog>> = Mutex::new(Weak::new());

/// The providers created by [`recording_net_provider`]
static EMBEDDER_PROVIDERS: Mutex<Vec<Weak<RecordingNetProvider>>> = Mutex::new(Vec::new());

impl NetworkLog {
    pub(crate) fn new(waker: Arc<dyn DevtoolsWaker>) -> Self {
        NetworkLog {
            records: Mutex::new(Vec::new()),
            active: AtomicBool::new(false),
            woken: AtomicBool::new(false),
            next_request_id: AtomicU64::new(1),
            waker,
        }
    }

    pub(crate) fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
        if !active {
            self.records.lock().unwrap().clear();
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Make this the log that providers created by [`recording_net_provider`]
    /// record into
    pub(crate) fn make_current(self: &Arc<Self>) {
        *CURRENT_LOG.lock().unwrap() = Arc::downgrade(self);
    }

    /// Take all logged records
    pub(crate) fn take(&self) -> Vec<(usize, NetworkRecord)> {
        let mut records = self.records.lock().unwrap();
        self.woken.store(false, Ordering::Relaxed);
        std::mem::take(&mut *records)
    }

    /// Return taken records that can't be sent yet (e.g. those of a document
    /// that is still being built) to the front of the log
    pub(crate) fn put_back(&self, taken: Vec<(usize, NetworkRecord)>) {
        let mut records = self.records.lock().unwrap();
        records.splice(0..0, taken);
    }

    fn push(&self, doc_id: usize, record: NetworkRecord) {
        let mut records = self.records.lock().unwrap();
        records.push((doc_id, record));
        let woken = self.woken.swap(true, Ordering::Relaxed);
        drop(records);
        // Requests complete on network threads: wake the embedder so that
        // the records are sent promptly
        if !woken {
            self.waker.wake();
        }
    }

    /// Log the records of a document's load ahead of the document's other
    /// records (which were logged while it was being built)
    fn push_document_load(&self, doc_id: usize, load: Vec<NetworkRecord>) {
        let mut records = self.records.lock().unwrap();
        let index = records
            .iter()
            .position(|(id, _)| *id == doc_id)
            .unwrap_or(records.len());
        records.splice(
            index..index,
            load.into_iter().map(|record| (doc_id, record)),
        );
        let woken = self.woken.swap(true, Ordering::Relaxed);
        drop(records);
        if !woken {
            self.waker.wake();
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }
}

/// The log of the current server, if it is recording
fn current_log() -> Option<Arc<NetworkLog>> {
    CURRENT_LOG
        .lock()
        .unwrap()
        .upgrade()
        .filter(|log| log.is_active())
}

/// Wrap a [`NetProvider`] so that the requests of documents created with it
/// are recorded from the start (while a CDP session has the `Network` domain
/// enabled), including those made while the document is being built, before
/// the server can wrap its provider itself. Records into the most recently
/// created [`CdpServer`](crate::CdpServer).
pub fn recording_net_provider(inner: Arc<dyn NetProvider>) -> Arc<dyn NetProvider> {
    let provider = Arc::new(RecordingNetProvider { inner, log: None });
    let mut providers = EMBEDDER_PROVIDERS.lock().unwrap();
    providers.retain(|provider| provider.strong_count() > 0);
    providers.push(Arc::downgrade(&provider));
    provider
}

/// Whether a document's provider was created by [`recording_net_provider`]
/// (so must not be wrapped again)
pub(crate) fn is_embedder_recorder(provider: &Arc<dyn NetProvider>) -> bool {
    let ptr = Arc::as_ptr(provider) as *const ();
    EMBEDDER_PROVIDERS
        .lock()
        .unwrap()
        .iter()
        .any(|recorder| recorder.as_ptr() as *const () == ptr && recorder.strong_count() > 0)
}

/// Records the load of a document's source that the embedder fetches itself,
/// before the document exists (e.g. a browser loading a page). The load is
/// reported as a `Document` request of the document once it is created
/// ([`finish`](Self::finish)), ahead of the requests it made while being built.
///
/// Does nothing unless a CDP session has the `Network` domain enabled when
/// the load starts.
pub struct DocumentLoadRecorder {
    log: Option<Arc<NetworkLog>>,
    request_id: u64,
    records: Vec<NetworkRecord>,
    signal: Option<AbortSignal>,
}

impl DocumentLoadRecorder {
    /// Record the start of the fetch of a document's source
    pub fn start(request: &Request) -> Self {
        let Some(log) = current_log() else {
            return DocumentLoadRecorder {
                log: None,
                request_id: 0,
                records: Vec::new(),
                signal: None,
            };
        };
        let request_id = log.next_request_id();
        let mut record = request_record(request_id, request);
        record.resource_type = ResourceType::Document;
        DocumentLoadRecorder {
            log: Some(log),
            request_id,
            records: vec![NetworkRecord::RequestWillBeSent(record)],
            signal: request.signal.clone(),
        }
    }

    /// Record the successful response to the fetch
    pub fn response(&mut self, resolved_url: &str, bytes: &Bytes) {
        if self.log.is_none() {
            return;
        }
        self.records
            .push(NetworkRecord::ResponseReceived(ResponseReceived {
                request_id: self.request_id,
                url: resolved_url.to_string(),
                mime_type: guess_mime_type(resolved_url, bytes).unwrap_or("text/html"),
                resource_type: ResourceType::Document,
                body: bytes.clone(),
                timestamp: monotonic_time(),
            }));
    }

    /// Record the failure of the fetch
    pub fn failed(&mut self, error: &str) {
        if self.log.is_none() {
            return;
        }
        self.records
            .push(NetworkRecord::LoadingFailed(LoadingFailed {
                request_id: self.request_id,
                resource_type: ResourceType::Document,
                error_text: error.to_string(),
                canceled: self.signal.as_ref().is_some_and(|signal| signal.aborted()),
                timestamp: monotonic_time(),
            }));
    }

    /// Report the load as a request of the document with the given id (the
    /// document created from the fetched source, or the error page shown
    /// instead)
    pub fn finish(self, doc_id: usize) {
        if let Some(log) = self.log
            && log.is_active()
        {
            log.push_document_load(doc_id, self.records);
        }
    }
}

/// The `RequestWillBeSent` record of a request
fn request_record(request_id: u64, request: &Request) -> RequestWillBeSent {
    let resource_type = guess_mime_type(request.url.as_str(), &[])
        .map(ResourceType::from_mime_type)
        .unwrap_or(ResourceType::Other);
    let mut headers: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|(name, value)| {
            let value = String::from_utf8_lossy(value.as_bytes()).into_owned();
            (name.to_string(), value)
        })
        .collect();
    if let Some(content_type) = &request.content_type {
        headers.push(("Content-Type".to_string(), content_type.clone()));
    }
    RequestWillBeSent {
        request_id,
        url: request.url.to_string(),
        method: request.method.to_string(),
        headers,
        resource_type,
        timestamp: monotonic_time(),
        wall_time: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_secs_f64())
            .unwrap_or(0.0),
    }
}

/// A [`NetProvider`] that forwards requests to the provider it wraps,
/// logging each request and its outcome while recording is active
pub(crate) struct RecordingNetProvider {
    pub(crate) inner: Arc<dyn NetProvider>,
    /// The log to record into (that of the current server if `None`)
    log: Option<Arc<NetworkLog>>,
}

impl RecordingNetProvider {
    pub(crate) fn new(inner: Arc<dyn NetProvider>, log: Arc<NetworkLog>) -> Self {
        RecordingNetProvider {
            inner,
            log: Some(log),
        }
    }
}

impl NetProvider for RecordingNetProvider {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let log = match &self.log {
            Some(log) => Some(Arc::clone(log)).filter(|log| log.is_active()),
            None => current_log(),
        };
        let Some(log) = log else {
            return self.inner.fetch(doc_id, request, handler);
        };
        let request_id = log.next_request_id();
        let record = request_record(request_id, &request);
        let resource_type = record.resource_type;
        log.push(doc_id, NetworkRecord::RequestWillBeSent(record));
        let handler = RecordingHandler {
            inner: handler,
            log,
            doc_id,
            request_id,
            resource_type,
            signal: request.signal.clone(),
        };
        self.inner.fetch(doc_id, request, Box::new(handler));
    }

    fn is_noop(&self) -> bool {
        self.inner.is_noop()
    }
}

struct RecordingHandler {
    inner: Box<dyn NetHandler>,
    log: Arc<NetworkLog>,
    doc_id: usize,
    request_id: u64,
    resource_type: ResourceType,
    signal: Option<AbortSignal>,
}

impl NetHandler for RecordingHandler {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        if self.log.is_active() {
            let mime_type =
                guess_mime_type(&resolved_url, &bytes).unwrap_or("application/octet-stream");
            let resource_type = match ResourceType::from_mime_type(mime_type) {
                ResourceType::Other => self.resource_type,
                sniffed => sniffed,
            };
            self.log.push(
                self.doc_id,
                NetworkRecord::ResponseReceived(ResponseReceived {
                    request_id: self.request_id,
                    url: resolved_url.clone(),
                    mime_type,
                    resource_type,
                    body: bytes.clone(),
                    timestamp: monotonic_time(),
                }),
            );
        }
        self.inner.bytes(resolved_url, bytes);
    }

    fn error(self: Box<Self>, url: String, error: String) {
        if self.log.is_active() {
            self.log.push(
                self.doc_id,
                NetworkRecord::LoadingFailed(LoadingFailed {
                    request_id: self.request_id,
                    resource_type: self.resource_type,
                    error_text: error.clone(),
                    canceled: self.signal.as_ref().is_some_and(|signal| signal.aborted()),
                    timestamp: monotonic_time(),
                }),
            );
        }
        self.inner.error(url, error);
    }
}

/// Seconds since an arbitrary (process-wide) point in time, as used for CDP
/// `MonotonicTime` timestamps
pub(crate) fn monotonic_time() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Guess the MIME type of a resource from its URL (the media type of a
/// `data:` URL, or the path's extension) and leading bytes
pub(crate) fn guess_mime_type(url: &str, bytes: &[u8]) -> Option<&'static str> {
    if let Some(mime_type) = sniff_mime_type(bytes) {
        return Some(mime_type);
    }
    if let Some(data) = url.strip_prefix("data:") {
        let media_type = data.split([',', ';']).next().unwrap_or("");
        return known_mime_type(&media_type.to_ascii_lowercase());
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let file_name = path.rsplit('/').next().unwrap_or("");
    let extension = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
    let mime_type = match extension.as_str() {
        "html" | "htm" => "text/html",
        "xhtml" => "application/xhtml+xml",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "txt" => "text/plain",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => return None,
    };
    Some(mime_type)
}

/// The static string for a known MIME type
fn known_mime_type(mime_type: &str) -> Option<&'static str> {
    const KNOWN: &[&str] = &[
        "text/html",
        "text/css",
        "text/plain",
        "text/javascript",
        "application/javascript",
        "application/json",
        "application/xml",
        "image/svg+xml",
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/avif",
        "font/woff",
        "font/woff2",
        "font/ttf",
        "font/otf",
    ];
    KNOWN.iter().copied().find(|known| *known == mime_type)
}

/// Identify a binary format from its magic bytes
fn sniff_mime_type(bytes: &[u8]) -> Option<&'static str> {
    const SIGNATURES: &[(&[u8], &str)] = &[
        (b"\x89PNG\r\n\x1a\n", "image/png"),
        (b"\xff\xd8\xff", "image/jpeg"),
        (b"GIF87a", "image/gif"),
        (b"GIF89a", "image/gif"),
        (b"wOFF", "font/woff"),
        (b"wOF2", "font/woff2"),
        (b"OTTO", "font/otf"),
        (b"\x00\x01\x00\x00", "font/ttf"),
    ];
    if let Some((_, mime_type)) = SIGNATURES
        .iter()
        .find(|(signature, _)| bytes.starts_with(signature))
    {
        return Some(mime_type);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// Whether a response with the given MIME type is text (sent to the
/// frontend as a string rather than base64-encoded)
pub(crate) fn is_text_mime_type(mime_type: &str) -> bool {
    mime_type.starts_with("text/")
        || matches!(
            mime_type,
            "application/json"
                | "application/javascript"
                | "application/xml"
                | "application/xhtml+xml"
                | "image/svg+xml"
        )
}

/// Standard (padded) base64 encoding
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn mime_types() {
        assert_eq!(
            guess_mime_type("https://a.b/style.css?v=1", b""),
            Some("text/css")
        );
        assert_eq!(
            guess_mime_type("https://a.b/img", b"\x89PNG\r\n\x1a\n.."),
            Some("image/png")
        );
        assert_eq!(
            guess_mime_type("data:image/svg+xml;base64,AAAA", b"<svg"),
            Some("image/svg+xml")
        );
        assert_eq!(guess_mime_type("https://a.b/", b"hello"), None);
    }
}
