//! Network activity recording. While a CDP session has the `Network` domain
//! enabled, the [`NetProvider`] of each document it inspects is wrapped in a
//! [`RecordingNetProvider`], which logs each request and its outcome; the
//! server later translates the logged records into `Network` domain events.
//!
//! `NetHandler` only reports the final URL and body of a successful response
//! (or an error message), so responses are reported with status 200 and no
//! headers, and their MIME type is guessed from the URL and body.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
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

/// Network records logged for all recorded documents (keyed by the id of
/// the document that issued the request)
pub(crate) struct NetworkLog {
    records: Mutex<Vec<(usize, NetworkRecord)>>,
    /// Whether any session has the `Network` domain enabled. Providers
    /// wrapped while recording (e.g. inherited by sub-documents created in
    /// the meantime) stop recording when this is cleared.
    active: AtomicBool,
    next_request_id: AtomicU64,
    waker: Arc<dyn DevtoolsWaker>,
}

impl NetworkLog {
    pub(crate) fn new(waker: Arc<dyn DevtoolsWaker>) -> Self {
        NetworkLog {
            records: Mutex::new(Vec::new()),
            active: AtomicBool::new(false),
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

    /// Take all logged records
    pub(crate) fn take(&self) -> Vec<(usize, NetworkRecord)> {
        std::mem::take(&mut *self.records.lock().unwrap())
    }

    fn push(&self, doc_id: usize, record: NetworkRecord) {
        let mut records = self.records.lock().unwrap();
        let was_empty = records.is_empty();
        records.push((doc_id, record));
        drop(records);
        // Requests complete on network threads: wake the embedder so that
        // the records are sent promptly
        if was_empty {
            self.waker.wake();
        }
    }
}

/// A [`NetProvider`] that forwards requests to the provider it wraps,
/// logging each request and its outcome while recording is active
pub(crate) struct RecordingNetProvider {
    pub(crate) inner: Arc<dyn NetProvider>,
    log: Arc<NetworkLog>,
}

impl RecordingNetProvider {
    pub(crate) fn new(inner: Arc<dyn NetProvider>, log: Arc<NetworkLog>) -> Self {
        RecordingNetProvider { inner, log }
    }
}

impl NetProvider for RecordingNetProvider {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        if !self.log.is_active() {
            return self.inner.fetch(doc_id, request, handler);
        }
        let request_id = self.log.next_request_id.fetch_add(1, Ordering::Relaxed);
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
        self.log.push(
            doc_id,
            NetworkRecord::RequestWillBeSent(RequestWillBeSent {
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
            }),
        );
        let handler = RecordingHandler {
            inner: handler,
            log: Arc::clone(&self.log),
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
fn monotonic_time() -> f64 {
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
