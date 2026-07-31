//! Abstractions of networking so that custom networking implementations can be provided

pub use bytes::Bytes;
pub use http::{self, HeaderMap, Method};
use serde::{
    Serialize,
    ser::{SerializeSeq, SerializeTuple},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{fmt, ops::Deref, path::PathBuf, sync::RwLock};
pub use url::Url;

/// A type that fetches resources for a Document.
///
/// This may be over the network via http(s), via the filesystem, or some other method.
pub trait NetProvider: Send + Sync + 'static {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>);
}

/// A type that parses raw bytes from a network request into a Data and then calls
/// the NetCallack with the result.
pub trait NetHandler: Send + Sync + 'static {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes);
}

/// An error produced while fetching bytes.
#[derive(Debug, Clone)]
pub enum FetchError {
    /// The request was aborted before or during fetching.
    Aborted,
    /// The fetcher reported an error message.
    Message(String),
}

impl FetchError {
    /// Creates an error from any displayable error or message.
    pub fn new(message: impl fmt::Display) -> Self {
        Self::Message(message.to_string())
    }

    /// Returns whether this error represents an aborted request.
    pub fn is_aborted(&self) -> bool {
        matches!(self, Self::Aborted)
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Aborted => f.write_str("request aborted"),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for FetchError {}

/// A callback-based source of fetched bytes.
pub trait ByteFetcher: Send + Sync + 'static {
    /// Fetches bytes and invokes `on_done` when the operation completes.
    #[allow(clippy::type_complexity)]
    fn fetch_bytes(
        &self,
        request: Request,
        on_done: Box<dyn FnOnce(Result<(String, Bytes), FetchError>) + Send>,
    );
}

impl<F: ByteFetcher> ByteFetcher for Arc<F> {
    fn fetch_bytes(
        &self,
        request: Request,
        on_done: Box<dyn FnOnce(Result<(String, Bytes), FetchError>) + Send>,
    ) {
        (**self).fetch_bytes(request, on_done);
    }
}

/// Observes requests and responses handled by a [`FetcherProvider`].
pub trait RequestObserver: Send + Sync + 'static {
    /// Called before a request is dispatched.
    fn on_request(&self, doc_id: usize, request: &Request);
    /// Called when a request completes, including when it fails.
    fn on_response(&self, doc_id: usize, url: &str, result: Result<&Bytes, &FetchError>);
}

/// Adapts a [`ByteFetcher`] to the [`NetProvider`] interface.
///
/// The waker fires before the [`NetHandler`] is invoked. Requests already
/// aborted at dispatch time are reported to the observer, but are not
/// dispatched or woken. The observer receives errors that cannot be delivered
/// through the [`NetHandler`] path.
pub struct FetcherProvider<F: ByteFetcher> {
    fetcher: F,
    waker: Arc<dyn NetWaker>,
    observer: RwLock<Option<Arc<dyn RequestObserver>>>,
}

impl<F: ByteFetcher> FetcherProvider<F> {
    /// Creates a provider with an optional request-completion waker.
    pub fn new(fetcher: F, waker: Option<Arc<dyn NetWaker>>) -> Self {
        Self {
            fetcher,
            waker: waker.unwrap_or_else(|| Arc::new(DummyNetWaker)),
            observer: RwLock::new(None),
        }
    }

    /// Returns the underlying byte fetcher.
    pub fn fetcher(&self) -> &F {
        &self.fetcher
    }

    /// Returns the provider's request-completion waker.
    pub fn waker(&self) -> &Arc<dyn NetWaker> {
        &self.waker
    }

    /// Installs the observer used for subsequent requests and responses.
    pub fn set_observer(&self, observer: Arc<dyn RequestObserver>) {
        *self
            .observer
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(observer);
    }
}

/// Dispatches byte fetches, waking before invoking the handler and exposing
/// failures through the observer.
impl<F: ByteFetcher> NetProvider for FetcherProvider<F> {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let signal = request.signal.clone();
        let url = request.url.to_string();
        let observer = self
            .observer
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if let Some(observer) = &observer {
            observer.on_request(doc_id, &request);
        }

        if signal.as_ref().is_some_and(AbortSignal::aborted) {
            if let Some(observer) = observer {
                let error = FetchError::Aborted;
                observer.on_response(doc_id, &url, Err(&error));
            }
            return;
        }

        let waker = self.waker.clone();
        self.fetcher.fetch_bytes(
            request,
            Box::new(move |mut result| {
                if signal.as_ref().is_some_and(AbortSignal::aborted) {
                    result = Err(FetchError::Aborted);
                }
                waker.wake(doc_id);
                if let Some(observer) = observer {
                    match &result {
                        Ok((response_url, bytes)) => {
                            observer.on_response(doc_id, response_url, Ok(bytes));
                        }
                        Err(error) => observer.on_response(doc_id, &url, Err(error)),
                    }
                }
                if let Ok((response_url, bytes)) = result {
                    handler.bytes(response_url, bytes);
                }
            }),
        );
    }
}

/// A callback which gets called every time a network request completes
// Q: Should we use std::task::Waker for this?
pub trait NetWaker: Send + Sync + 'static {
    fn wake(&self, client_id: usize);
}

impl<F: Fn(usize) + Send + Sync + 'static> NetWaker for F {
    fn wake(&self, doc_id: usize) {
        self(doc_id)
    }
}

struct DummyNetWaker;
impl NetWaker for DummyNetWaker {
    fn wake(&self, _client_id: usize) {}
}

#[non_exhaustive]
#[derive(Debug, Clone)]
/// A request type loosely representing <https://fetch.spec.whatwg.org/#requests>
pub struct Request {
    pub url: Url,
    pub method: Method,
    pub content_type: Option<String>,
    pub headers: HeaderMap,
    pub body: Body,
    pub signal: Option<AbortSignal>,
}
impl Request {
    /// A get request to the specified Url and an empty body
    pub fn get(url: Url) -> Self {
        Self {
            url,
            method: Method::GET,
            content_type: None,
            headers: HeaderMap::new(),
            body: Body::Empty,
            signal: None,
        }
    }

    pub fn signal(mut self, signal: AbortSignal) -> Self {
        self.signal = Some(signal);
        self
    }
}

#[derive(Debug, Clone)]
pub enum Body {
    Bytes(Bytes),
    Form(FormData),
    Empty,
}

/// A list of form entries used for form submission
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FormData(pub Vec<Entry>);
impl FormData {
    /// Creates a new empty FormData
    pub fn new() -> Self {
        FormData(Vec::new())
    }
}
impl Serialize for FormData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq_serializer = serializer.serialize_seq(Some(self.len()))?;
        for entry in &self.0 {
            seq_serializer.serialize_element(entry)?;
        }
        seq_serializer.end()
    }
}
impl Deref for FormData {
    type Target = Vec<Entry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A single form entry consisting of a name and value
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub value: EntryValue,
}
impl Serialize for Entry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut serializer = serializer.serialize_tuple(2)?;
        serializer.serialize_element(&self.name)?;
        match &self.value {
            EntryValue::String(s) => serializer.serialize_element(s)?,
            EntryValue::File(p) => serializer.serialize_element(p.to_str().unwrap_or_default())?,
            EntryValue::EmptyFile => serializer.serialize_element("")?,
        }
        serializer.end()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntryValue {
    String(String),
    File(PathBuf),
    EmptyFile,
}
impl AsRef<str> for EntryValue {
    fn as_ref(&self) -> &str {
        match self {
            EntryValue::String(s) => s,
            EntryValue::File(p) => p.to_str().unwrap_or_default(),
            EntryValue::EmptyFile => "",
        }
    }
}

impl From<&str> for EntryValue {
    fn from(value: &str) -> Self {
        EntryValue::String(value.to_string())
    }
}
impl From<PathBuf> for EntryValue {
    fn from(value: PathBuf) -> Self {
        EntryValue::File(value)
    }
}

/// A default noop NetProvider
#[derive(Default)]
pub struct DummyNetProvider;
impl NetProvider for DummyNetProvider {
    fn fetch(&self, _doc_id: usize, _request: Request, _handler: Box<dyn NetHandler>) {}
}

/// The AbortController interface represents a controller object that
/// allows you to abort one or more Web requests as and when desired.
///
/// <https://developer.mozilla.org/en-US/docs/Web/API/AbortController>
#[derive(Debug, Default)]
pub struct AbortController {
    pub signal: AbortSignal,
}

impl AbortController {
    /// The abort() method of the AbortController interface aborts
    /// an asynchronous operation before it has completed.
    /// This is able to abort fetch requests.
    ///
    /// <https://developer.mozilla.org/en-US/docs/Web/API/AbortController/abort>
    pub fn abort(self) {
        self.signal.0.store(true, Ordering::SeqCst);
    }
}

/// The AbortSignal interface represents a signal object that allows you to
/// communicate with an asynchronous operation (such as a fetch request) and
/// abort it if required via an AbortController object.
///
/// <https://developer.mozilla.org/en-US/docs/Web/API/AbortSignal>
#[derive(Debug, Default, Clone)]
pub struct AbortSignal(Arc<AtomicBool>);

impl AbortSignal {
    /// The aborted read-only property returns a value that indicates whether
    /// the asynchronous operations the signal is communicating with are
    /// aborted (true) or not (false).
    ///
    /// <https://developer.mozilla.org/en-US/docs/Web/API/AbortSignal/aborted>
    pub fn aborted(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, atomic::AtomicUsize};

    struct SyncFetcher {
        result: Result<(String, Bytes), FetchError>,
        calls: Arc<AtomicUsize>,
    }

    impl ByteFetcher for SyncFetcher {
        fn fetch_bytes(
            &self,
            _request: Request,
            on_done: Box<dyn FnOnce(Result<(String, Bytes), FetchError>) + Send>,
        ) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            on_done(self.result.clone());
        }
    }

    struct Handler(Arc<AtomicBool>);
    impl NetHandler for Handler {
        fn bytes(self: Box<Self>, _resolved_url: String, _bytes: Bytes) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    struct Observer {
        responses: Mutex<Vec<bool>>,
    }

    impl RequestObserver for Observer {
        fn on_request(&self, _doc_id: usize, _request: &Request) {}

        fn on_response(&self, _doc_id: usize, _url: &str, result: Result<&Bytes, &FetchError>) {
            self.responses
                .lock()
                .unwrap()
                .push(result.is_err() && result.unwrap_err().is_aborted());
        }
    }

    fn request() -> Request {
        Request::get(Url::parse("https://example.com/").unwrap())
    }

    #[test]
    fn synchronous_fetcher_completes_before_fetch_returns() {
        let ran = Arc::new(AtomicBool::new(false));
        let fetcher = SyncFetcher {
            result: Ok(("https://example.com/".into(), Bytes::from_static(b"ok"))),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let provider = FetcherProvider::new(fetcher, None);
        provider.fetch(1, request(), Box::new(Handler(ran.clone())));
        assert!(ran.load(Ordering::SeqCst));
    }

    #[test]
    fn fetch_error_reaches_observer() {
        let observer = Arc::new(Observer {
            responses: Mutex::new(Vec::new()),
        });
        let fetcher = SyncFetcher {
            result: Err(FetchError::new("failed")),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let provider = FetcherProvider::new(fetcher, None);
        provider.set_observer(observer.clone());
        provider.fetch(
            1,
            request(),
            Box::new(Handler(Arc::new(AtomicBool::new(false)))),
        );
        assert_eq!(*observer.responses.lock().unwrap(), vec![false]);
    }

    #[test]
    fn aborted_request_reaches_observer_without_calling_handler() {
        let observer = Arc::new(Observer {
            responses: Mutex::new(Vec::new()),
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = SyncFetcher {
            result: Ok(("https://example.com/".into(), Bytes::from_static(b"ok"))),
            calls: calls.clone(),
        };
        let provider = FetcherProvider::new(fetcher, None);
        provider.set_observer(observer.clone());
        let controller = AbortController::default();
        let signal = controller.signal.clone();
        controller.abort();
        provider.fetch(
            1,
            request().signal(signal),
            Box::new(Handler(Arc::new(AtomicBool::new(false)))),
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(*observer.responses.lock().unwrap(), vec![true]);
    }
}
